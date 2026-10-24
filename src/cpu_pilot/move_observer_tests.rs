use super::*;

fn sample(game_frame: u64, status: i32, prev0: i32) -> Observation {
    Observation {
        game_frame,
        status,
        prev0,
        p1_damage: 0.0,
        native_interrupted: None,
    }
}

fn completed_trace(statuses: &[i32], expected: Family) {
    let mut observer = MoveObserver::default();
    let mut completed = Vec::new();
    for (tick, &status) in statuses.iter().enumerate() {
        let event = observer.step(sample(tick as u64, status, 0));
        assert!(event.interrupted.is_none());
        completed.extend(event.completed);
    }
    assert_eq!(completed, vec![expected]);
    assert_eq!(observer.active_family(), None);
}

#[test]
fn jab_combo_and_charged_smash_phases_complete_once() {
    completed_trace(&[0x27, 0x27, 0x28, 0x28, 0], Family::Jab);
    completed_trace(&[0x2d, 0x2e, 0x2e, 0x2f, 0], Family::SideSmash);
}

#[test]
fn aerial_and_special_aftermath_and_extended_phases_continue_one_instance() {
    completed_trace(&[0x36, 0x18, 0x18, 0], Family::Aerial);
    completed_trace(&[0x1de, 0x10, 0x19, 0], Family::UpSpecial);
    completed_trace(&[0x1df, 0x1e1, 0x1e2, 0x1e2, 0], Family::DownSpecial);
}

#[test]
fn whiffs_complete_and_family_cancel_closes_then_opens() {
    let mut observer = MoveObserver::default();
    assert_eq!(
        observer.step(sample(0, 0x27, 0)).active_family,
        Some(Family::Jab)
    );
    // No hit/damage signal is required: this completed attack can have whiffed.
    let cancel = observer.step(sample(1, 0x2a, 0x27));
    assert_eq!(cancel.completed, Some(Family::Jab));
    assert_eq!(cancel.active_family, Some(Family::SideTilt));
    assert_eq!(
        observer.step(sample(2, 0, 0x2a)).completed,
        Some(Family::SideTilt)
    );
}

#[test]
fn prev0_recovers_common_entry_but_unknown_specific_status_cannot_open_alone() {
    let mut observer = MoveObserver::default();
    assert_eq!(
        observer.step(sample(0, 0x1e1, 0x1df)).active_family,
        Some(Family::DownSpecial)
    );
    assert_eq!(
        observer.step(sample(1, 0, 0x1e1)).completed,
        Some(Family::DownSpecial)
    );
    assert_eq!(observer.step(sample(2, 0x203, 0x37)).active_family, None);
    assert_eq!(observer.step(sample(3, 0, 0x203)).completed, None);
}

#[test]
fn damage_in_extended_phase_interrupts_without_stale_reopening() {
    let mut observer = MoveObserver::default();
    observer.step(sample(0, 0x1df, 0));
    observer.step(sample(1, 0x1e1, 0x1df));
    let event = observer.step(Observation {
        p1_damage: 4.0,
        ..sample(2, 0x1e1, 0x1df)
    });
    assert_eq!(
        event.interrupted,
        Some(InterruptedMove {
            family: Family::DownSpecial,
            reason: InterruptionReason::DamageRiseUnconfirmed
        })
    );
    assert_eq!(event.completed, None);
    assert_eq!(event.active_family, None);
    let stale = observer.step(Observation {
        p1_damage: 4.0,
        ..sample(3, 0x1e1, 0x1df)
    });
    assert_eq!(stale, Events::default());
}

#[test]
fn explicit_native_signal_and_common_interruptions_are_separate_from_armor_policy() {
    let mut observer = MoveObserver::default();
    observer.step(sample(0, 0x27, 0));
    let native = observer.step(Observation {
        native_interrupted: Some(true),
        ..sample(1, 0x27, 0)
    });
    assert_eq!(
        native.interrupted.unwrap().reason,
        InterruptionReason::NativeSignal
    );
    assert_eq!(observer.step(sample(2, 0x27, 0)).active_family, None);
    observer.reset();
    observer.step(sample(3, 0x2d, 0));
    let armor = observer.step(Observation {
        p1_damage: 5.0,
        native_interrupted: Some(false),
        ..sample(4, 0x2e, 0x2d)
    });
    assert_eq!(armor.interrupted, None);
    assert_eq!(armor.active_family, Some(Family::SideSmash));
    let hitstun = observer.step(Observation {
        p1_damage: 5.0,
        native_interrupted: Some(false),
        ..sample(5, 0x47, 0x2e)
    });
    assert_eq!(
        hitstun.interrupted.unwrap().reason,
        InterruptionReason::CommonStatus
    );
    assert_eq!(hitstun.completed, None);
}

#[test]
fn grabs_throws_jumps_and_final_do_not_open_moves() {
    let mut observer = MoveObserver::default();
    for (tick, status) in [0x37, 0x38, 0x40, 0xb, 0xc, 0x1e0, 0].iter().enumerate() {
        assert_eq!(
            observer.step(sample(tick as u64, *status, 0)),
            Events::default()
        );
    }
}

#[test]
fn repeated_ticks_and_unobserved_same_status_reentry_cannot_invent_instances() {
    let mut observer = MoveObserver::default();
    observer.step(sample(10, 0x27, 0));
    assert_eq!(
        observer.step(sample(10, 0, 0x27)),
        Events {
            active_family: Some(Family::Jab),
            ..Events::default()
        }
    );
    // A new ATTACK instance with no observed transition is indistinguishable.
    assert_eq!(observer.step(sample(11, 0x27, 0x27)).completed, None);
    assert_eq!(
        observer.step(sample(12, 0, 0x27)).completed,
        Some(Family::Jab)
    );
    assert_eq!(observer.step(sample(12, 0, 0x27)), Events::default());
}
