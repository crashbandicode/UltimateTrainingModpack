use super::*;
use std::convert::TryFrom;

// Synthetic rank fixture only; never evidence of a native level/rank mapping.
const RANKS: Option<[u8; 9]> = Some([1; 9]);

fn cfg(trigger: Trigger, command: Command) -> Config {
    Config {
        level: 9,
        trigger,
        command,
        require_contact: true,
        guard_frames: 3600,
    }
}

fn live(game_frame: u64) -> Frame {
    Frame {
        game_frame,
        identity_matches: true,
        active: true,
        ..Frame::default()
    }
}

#[test]
fn parses_exact_config_examples_and_rounds_seconds_up() {
    let timed =
        config::from_json(include_str!("examples/fight-five-seconds-walk-off.json")).unwrap();
    assert_eq!(timed, cfg(Trigger::AfterGameFrames(300), Command::WalkOff));
    let moves =
        config::from_json(include_str!("examples/completed-attacks-walk-off.json")).unwrap();
    assert_eq!(
        moves,
        cfg(Trigger::AfterCompletedMoves(3), Command::WalkOff)
    );
    let release = config::from_json(include_str!("examples/immediate-release.json")).unwrap();
    assert_eq!(release.level, 1);
    assert_eq!(release.trigger, Trigger::Immediately);
    assert_eq!(release.command, Command::Release);
    let boundary = config::JsonConfig {
        level: 9,
        trigger: config::JsonTrigger::AfterSeconds { seconds: 0.0167 },
        command: config::JsonCommand::WalkOff,
        require_contact: true,
        guard_frames: 60,
    };
    assert_eq!(
        Config::try_from(boundary).unwrap().trigger,
        Trigger::AfterGameFrames(2)
    );
    let defaults = r#"{"level":9,"trigger":{"kind":"after_seconds","seconds":0},"command":"walk_off","guard_frames":60}"#;
    let parsed = config::from_json(defaults).unwrap();
    assert!(parsed.require_contact);
    assert_eq!(parsed.trigger, Trigger::AfterGameFrames(0));
}

#[test]
fn rejects_invalid_config_and_requires_measured_ranks() {
    for input in [
        r#"{"level":0,"trigger":{"kind":"immediate"},"command":"release","guard_frames":60}"#,
        r#"{"level":10,"trigger":{"kind":"immediate"},"command":"release","guard_frames":60}"#,
        r#"{"level":1,"trigger":{"kind":"after_moves","moves":-1},"command":"release","guard_frames":60}"#,
        r#"{"level":1,"trigger":{"kind":"after_seconds","seconds":-0.1},"command":"release","guard_frames":60}"#,
        r#"{"level":1,"trigger":{"kind":"immediate","seconds":2},"command":"release","guard_frames":60}"#,
    ] {
        assert!(config::from_json(input).is_err(), "{}", input);
    }
    let invalid = config::JsonConfig {
        level: 1,
        trigger: config::JsonTrigger::AfterSeconds {
            seconds: f64::INFINITY,
        },
        command: config::JsonCommand::Release,
        require_contact: true,
        guard_frames: 60,
    };
    assert!(Config::try_from(invalid).is_err());
    assert!(Pilot::new(cfg(Trigger::Immediately, Command::Release), None).is_err());
}

#[test]
fn immediate_and_zero_move_commands_obey_contact_configuration() {
    for trigger in [Trigger::Immediately, Trigger::AfterCompletedMoves(0)] {
        let mut release = Pilot::new(cfg(trigger, Command::Release), RANKS).unwrap();
        assert_eq!(release.step(live(10)), Output::Pad);
        assert_eq!(release.phase(), Phase::Done(End::Completed));
        let mut config = cfg(trigger, Command::WalkOff);
        config.require_contact = false;
        let mut walk = Pilot::new(config, RANKS).unwrap();
        assert_eq!(walk.step(live(10)), Output::Command(Command::WalkOff));
    }
}

#[test]
fn completed_attack_special_events_include_whiffs_without_guessing_or_duplicates() {
    let mut pilot = Pilot::new(
        cfg(Trigger::AfterCompletedMoves(2), Command::WalkOff),
        RANKS,
    )
    .unwrap();
    assert_eq!(pilot.step(live(0)), Output::Ai);
    // Time / other actions alone cannot synthesize completed attack events.
    assert_eq!(pilot.step(live(100)), Output::Ai);
    assert_eq!(pilot.completed_moves(), 0);
    let one_whiff = Frame {
        completed_moves: 1,
        ..live(101)
    };
    assert_eq!(pilot.step(one_whiff), Output::Ai);
    assert_eq!(pilot.step(one_whiff), Output::Ai);
    assert_eq!(pilot.completed_moves(), 1);
    assert_eq!(
        pilot.step(Frame {
            completed_moves: 1,
            ..live(102)
        }),
        Output::Ai
    );
    assert_eq!(pilot.completed_moves(), 2);
    assert_eq!(pilot.damaging_hits(), 0);
    assert_eq!(
        pilot.step(Frame {
            infliction: true,
            target_damage: 6.0,
            ..live(103)
        }),
        Output::Command(Command::WalkOff)
    );
}

#[test]
fn game_time_uses_active_frame_deltas_and_pauses_return_physical_pad() {
    let mut config = cfg(Trigger::AfterGameFrames(300), Command::WalkOff);
    config.require_contact = false;
    let mut pilot = Pilot::new(config, RANKS).unwrap();
    assert_eq!(pilot.step(live(1000)), Output::Ai);
    assert_eq!(pilot.elapsed_game_frames(), 0);
    assert_eq!(pilot.step(live(1001)), Output::Ai);
    assert_eq!(
        pilot.step(Frame {
            completed_moves: 9,
            ..live(1001)
        }),
        Output::Ai
    );
    assert_eq!(
        (pilot.elapsed_game_frames(), pilot.completed_moves()),
        (1, 0)
    );
    assert_eq!(
        pilot.step(Frame {
            active: false,
            ..live(2000)
        }),
        Output::Pad
    );
    assert_eq!(pilot.step(live(3000)), Output::Ai);
    assert_eq!(pilot.elapsed_game_frames(), 1);
    assert_eq!(pilot.step(live(3298)), Output::Ai);
    assert_eq!(pilot.elapsed_game_frames(), 299);
    assert_eq!(pilot.step(live(3299)), Output::Command(Command::WalkOff));
    assert_eq!(pilot.elapsed_game_frames(), 300);
    assert_eq!(pilot.completed_moves(), 0);
    assert_eq!(
        pilot.step(Frame {
            active: false,
            ..live(4000)
        }),
        Output::Pad
    );
}

#[test]
fn same_or_next_frame_damaging_hit_allows_walk_off() {
    for delayed in [false, true] {
        let mut pilot = Pilot::new(cfg(Trigger::Immediately, Command::WalkOff), RANKS).unwrap();
        assert_eq!(pilot.step(live(10)), Output::Ai);
        let edge = Frame {
            infliction: true,
            target_damage: if delayed { 0.0 } else { 8.0 },
            ..live(11)
        };
        if delayed {
            assert_eq!(pilot.step(edge), Output::Ai);
            assert_eq!(
                pilot.step(Frame {
                    target_damage: 8.0,
                    ..live(12)
                }),
                Output::Command(Command::WalkOff)
            );
        } else {
            assert_eq!(pilot.step(edge), Output::Command(Command::WalkOff));
        }
        assert_eq!(pilot.damaging_hits(), 1);
        assert_eq!(pilot.damage_dealt(), 8.0);
    }
}

#[test]
fn sustained_infliction_cannot_count_damage_beyond_next_frame() {
    let mut pilot = Pilot::new(cfg(Trigger::Immediately, Command::WalkOff), RANKS).unwrap();
    assert_eq!(pilot.step(live(0)), Output::Ai);
    assert_eq!(
        pilot.step(Frame {
            infliction: true,
            ..live(1)
        }),
        Output::Ai
    );
    assert_eq!(
        pilot.step(Frame {
            infliction: true,
            ..live(2)
        }),
        Output::Ai
    );
    assert_eq!(
        pilot.step(Frame {
            infliction: true,
            target_damage: 7.0,
            ..live(3)
        }),
        Output::Ai
    );
    assert_eq!(pilot.damaging_hits(), 0);
    // A fresh later hit can still establish contact after the stale one expires.
    assert_eq!(
        pilot.step(Frame {
            target_damage: 7.0,
            ..live(4)
        }),
        Output::Ai
    );
    assert_eq!(
        pilot.step(Frame {
            infliction: true,
            target_damage: 10.0,
            ..live(5)
        }),
        Output::Command(Command::WalkOff)
    );
    assert_eq!(pilot.damage_dealt(), 3.0);
}

#[test]
fn pause_discards_pending_hit_before_unrelated_resumed_damage() {
    let mut pilot = Pilot::new(cfg(Trigger::Immediately, Command::WalkOff), RANKS).unwrap();
    pilot.step(live(0));
    pilot.step(Frame {
        infliction: true,
        ..live(1)
    });
    assert_eq!(
        pilot.step(Frame {
            active: false,
            ..live(2)
        }),
        Output::Pad
    );
    assert_eq!(
        pilot.step(Frame {
            target_damage: 12.0,
            ..live(100)
        }),
        Output::Ai
    );
    assert_eq!(pilot.damaging_hits(), 0);
}

#[test]
fn match_end_identity_and_walk_off_completion_release_control() {
    for identity_change in [false, true] {
        let mut pilot =
            Pilot::new(cfg(Trigger::AfterGameFrames(300), Command::Release), RANKS).unwrap();
        assert_eq!(pilot.step(live(10)), Output::Ai);
        let terminal = Frame {
            identity_matches: !identity_change,
            match_ended: !identity_change,
            ..live(10)
        };
        assert_eq!(pilot.step(terminal), Output::Pad);
        assert_eq!(
            pilot.phase(),
            Phase::Done(if identity_change {
                End::IdentityChanged
            } else {
                End::MatchEnded
            })
        );
        assert_eq!(pilot.step(live(11)), Output::Pad);
    }
    let mut config = cfg(Trigger::Immediately, Command::WalkOff);
    config.require_contact = false;
    let mut pilot = Pilot::new(config, RANKS).unwrap();
    assert_eq!(pilot.step(live(10)), Output::Command(Command::WalkOff));
    assert_eq!(
        pilot.step(Frame {
            p1_lost_stock: true,
            ..live(11)
        }),
        Output::Pad
    );
    assert_eq!(pilot.phase(), Phase::Done(End::Completed));
}

#[test]
fn guard_releases_both_ai_and_command_at_frame_limit() {
    for trigger in [Trigger::Immediately, Trigger::AfterGameFrames(60)] {
        let mut config = cfg(trigger, Command::WalkOff);
        config.require_contact = false;
        config.guard_frames = 2;
        let mut pilot = Pilot::new(config, RANKS).unwrap();
        pilot.step(live(100));
        pilot.step(live(101));
        assert_eq!(pilot.step(live(102)), Output::Pad);
        assert_eq!(pilot.phase(), Phase::Done(End::GuardExpired));
        assert_eq!(pilot.step(live(103)), Output::Pad);
    }
}
