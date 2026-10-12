use super::*;
use crate::test_util::{frames, recording, start};

#[test]
fn new_builds_a_valid_recording_with_default_placement() {
    let rec = MacroRecording::new("  Fox drill  ", "13.0.4", start(), frames(3)).unwrap();
    assert_eq!(rec.schema_version, SCHEMA_VERSION);
    assert_eq!(rec.name, "Fox drill", "name is trimmed");
    assert_eq!(rec.placement, Placement::default());
    assert_eq!(rec.placement.momentum, MomentumMode::PreserveRecorded);
    assert_eq!((rec.placement.delta_x, rec.placement.delta_y), (0.0, 0.0));
}

#[test]
fn json_round_trip_is_exact_and_uses_stable_field_names() {
    let mut rec = recording("Round trip");
    rec.frames[0] = InputFrame {
        buttons: u32::MAX,
        lstick_x: i8::MIN,
        lstick_y: i8::MAX,
        rstick_x: -1,
        rstick_y: 0,
    };
    rec.set_placement(Placement {
        delta_x: -12.5,
        delta_y: 30.25,
        momentum: MomentumMode::AtRest,
    })
    .unwrap();

    let json = serde_json::to_string(&rec).unwrap();
    assert!(json.contains("\"momentum\":\"at_rest\""), "{json}");
    assert!(json.contains("\"schema_version\":1"), "{json}");
    let back: MacroRecording = serde_json::from_str(&json).unwrap();
    assert_eq!(back, rec);
    back.validate().unwrap();
}

#[test]
fn rejects_empty_and_oversized_frame_lists() {
    let err = MacroRecording::new("Empty", "", start(), Vec::new()).unwrap_err();
    assert!(matches!(err, MacroError::EmptyFrames), "{err}");

    let at_limit = MacroRecording::new("Limit", "", start(), frames(MAX_FRAMES));
    assert!(at_limit.is_ok(), "exactly MAX_FRAMES is accepted");

    let err = MacroRecording::new("Big", "", start(), frames(MAX_FRAMES + 1)).unwrap_err();
    assert!(
        matches!(err, MacroError::TooManyFrames { frames, max } if frames == MAX_FRAMES + 1 && max == MAX_FRAMES),
        "{err}"
    );
}

#[test]
fn name_rules() {
    for bad in ["", "   ", "\t", "bad\nname", "nul\0name"] {
        let err = MacroRecording::new(bad, "", start(), frames(1)).unwrap_err();
        assert!(matches!(err, MacroError::InvalidName(_)), "{bad:?}: {err}");
    }
    let too_long = "x".repeat(MAX_NAME_CHARS + 1);
    let err = MacroRecording::new(too_long, "", start(), frames(1)).unwrap_err();
    assert!(
        matches!(err, MacroError::InvalidName("is too long")),
        "{err}"
    );

    // The limit counts characters, not bytes.
    let multibyte = "é".repeat(MAX_NAME_CHARS);
    assert!(MacroRecording::new(multibyte, "", start(), frames(1)).is_ok());
    assert!(MacroRecording::new("x".repeat(MAX_NAME_CHARS), "", start(), frames(1)).is_ok());
}

#[test]
fn set_name_trims_validates_and_leaves_frames_alone() {
    let mut rec = recording("Before");
    let frames_before = rec.frames.clone();
    rec.set_name("  After  ").unwrap();
    assert_eq!(rec.name, "After");
    assert!(rec.set_name("   ").is_err());
    assert_eq!(rec.name, "After", "a rejected rename changes nothing");
    assert_eq!(rec.frames, frames_before);
}

#[test]
fn game_version_rules() {
    assert!(MacroRecording::new("Ok", "", start(), frames(1)).is_ok());
    let long = "9".repeat(MAX_GAME_VERSION_BYTES + 1);
    let err = MacroRecording::new("V", long, start(), frames(1)).unwrap_err();
    assert!(matches!(err, MacroError::InvalidGameVersion(_)), "{err}");
    let err = MacroRecording::new("V", "1.0\n", start(), frames(1)).unwrap_err();
    assert!(matches!(err, MacroError::InvalidGameVersion(_)), "{err}");
}

type SetField = fn(&mut MacroRecording, f32);

#[test]
fn rejects_non_finite_floats_in_every_field() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mutations: [(&str, SetField); 5] = [
            ("start.facing", |r, v| r.start.facing = v),
            ("start.speed_x", |r, v| r.start.speed_x = v),
            ("start.speed_y", |r, v| r.start.speed_y = v),
            ("placement.delta_x", |r, v| r.placement.delta_x = v),
            ("placement.delta_y", |r, v| r.placement.delta_y = v),
        ];
        for (field, mutate) in mutations {
            let mut rec = recording("Float");
            mutate(&mut rec, bad);
            let err = rec.validate().unwrap_err();
            assert!(
                matches!(err, MacroError::NonFinite(f) if f == field),
                "{field} {bad}: {err}"
            );
        }
    }
}

#[test]
fn rejects_unsupported_schema_version_and_missing_fighter() {
    for version in [0, 2, u32::MAX] {
        let mut rec = recording("Version");
        rec.schema_version = version;
        let err = rec.validate().unwrap_err();
        assert!(
            matches!(err, MacroError::UnsupportedVersion { found } if found == version),
            "{err}"
        );
    }

    let err = MacroRecording::new("Nobody", "", StartState::default(), frames(1)).unwrap_err();
    assert!(matches!(err, MacroError::MissingFighter), "{err}");
    assert!(MacroRecording::new("Mario", "", StartState::neutral(0, 0, 1.0), frames(1)).is_ok());
}

#[test]
fn neutral_start_has_identity_and_no_motion() {
    let state = StartState::neutral(8, 3, -1.0);
    assert_eq!(
        (state.fighter_kind, state.costume, state.facing),
        (8, 3, -1.0)
    );
    assert_eq!((state.situation, state.status), (0, 0));
    assert_eq!((state.speed_x, state.speed_y), (0.0, 0.0));
}

#[test]
fn placement_edit_is_independent_of_frames_and_start() {
    let mut rec = recording("Edit");
    let (frames_before, start_before) = (rec.frames.clone(), rec.start);

    let edit = Placement {
        delta_x: 4.0,
        delta_y: 55.5,
        momentum: MomentumMode::AtRest,
    };
    rec.set_placement(edit).unwrap();
    assert_eq!(rec.placement, edit);
    assert_eq!(rec.frames, frames_before);
    assert_eq!(rec.start, start_before);

    let bad = Placement {
        delta_y: f32::NAN,
        ..edit
    };
    assert!(matches!(
        rec.set_placement(bad),
        Err(MacroError::NonFinite("placement.delta_y"))
    ));
    assert_eq!(
        rec.placement, edit,
        "a rejected edit leaves the old placement"
    );
}

#[test]
fn placement_is_relative_to_the_anchor_and_never_compounds() {
    let placement = Placement {
        delta_x: -10.0,
        delta_y: 25.0,
        momentum: MomentumMode::PreserveRecorded,
    };
    // Each trial restores the anchor first, so every trial lands on the same spot.
    let anchor = (100.0, -4.0);
    let first = placement.apply(anchor);
    let second = placement.apply(anchor);
    assert_eq!(first, (90.0, 21.0));
    assert_eq!(first, second);
    // A different restored anchor shifts the result by exactly the same delta.
    assert_eq!(placement.apply((0.0, 0.0)), (-10.0, 25.0));
}
