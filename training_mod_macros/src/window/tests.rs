use super::*;
use crate::{
    test_util::{frames, TempDir},
    Library, MomentumMode, MAX_FRAMES,
};

fn window(total: u32) -> FrameWindow {
    FrameWindow::new(total).unwrap()
}

/// What the runtime would report at the in-point: moving, mid-air.
fn snapshot_at(frame: u32) -> FrameSnapshot {
    FrameSnapshot {
        frame,
        state: StartState {
            fighter_kind: 8,
            costume: 1,
            facing: 1.0,
            situation: 3,
            status: 42,
            speed_x: 0.75,
            speed_y: 2.5,
        },
    }
}

#[test]
fn step_sizes_follow_the_supplied_fps() {
    assert_eq!(Step::Frame.frames(60), 1);
    assert_eq!(Step::Frame.frames(30), 1);
    assert_eq!(Step::SECOND.frames(60), 60);
    assert_eq!(Step::FIVE_SECONDS.frames(60), 300);
    assert_eq!(Step::Seconds(3).frames(60), 180);
    assert_eq!(Step::Seconds(3).frames(30), 90);
    assert_eq!(Step::Seconds(2).frames(120), 240);
    assert_eq!(Step::Seconds(0).frames(60), 0);
    assert_eq!(Step::Seconds(u32::MAX).frames(60), u32::MAX, "saturates");
    assert_eq!(window(10).fps(), DEFAULT_FPS);
}

#[test]
fn cursor_steps_exactly_and_clamps_at_both_ends() {
    let mut w = window(1000);
    assert_eq!(w.back(Step::SECOND), 0, "clamped at the start");
    assert_eq!(w.forward(Step::Frame), 1);
    assert_eq!(w.forward(Step::SECOND), 61);
    assert_eq!(w.forward(Step::FIVE_SECONDS), 361);
    assert_eq!(w.forward(Step::Seconds(2)), 481);
    assert_eq!(w.back(Step::Seconds(2)), 361);
    assert_eq!(w.back(Step::Frame), 360);
    assert_eq!(w.forward(Step::FIVE_SECONDS), 660);
    assert_eq!(w.forward(Step::FIVE_SECONDS), 960);
    assert_eq!(
        w.forward(Step::FIVE_SECONDS),
        1000,
        "clamped at the end boundary"
    );
    assert_eq!(w.forward(Step::Frame), 1000, "stays at the end");
    assert_eq!(w.back(Step::Frame), 999);
    assert_eq!(w.back(Step::Seconds(u32::MAX)), 0);
    assert_eq!(w.forward(Step::Seconds(u32::MAX)), 1000);
    assert_eq!(w.forward(Step::Seconds(0)), 1000);
}

#[test]
fn custom_fps_changes_what_a_second_means() {
    let mut w = FrameWindow::with_fps(1000, 30).unwrap();
    assert_eq!(w.forward(Step::SECOND), 30);
    assert_eq!(w.forward(Step::Seconds(4)), 150);
    assert_eq!(w.back(Step::FIVE_SECONDS), 0);
}

#[test]
fn seek_clamps_to_the_boundary_range() {
    let mut w = window(500);
    assert_eq!(w.seek(250), 250);
    assert_eq!(
        w.seek(500),
        500,
        "position `total` is the end of the source"
    );
    assert_eq!(w.seek(501), 500);
    assert_eq!(w.seek(u32::MAX), 500);
    assert_eq!(w.seek(0), 0);
}

#[test]
fn a_new_window_selects_the_whole_source() {
    let w = window(240);
    assert_eq!(
        (w.cursor(), w.start(), w.end(), w.total()),
        (0, 0, 240, 240)
    );
    assert_eq!(w.range(), 0..240);
    assert_eq!(w.selected_frames(), 240);
}

#[test]
fn selection_is_half_open_and_never_empty() {
    let mut w = window(600);
    assert_eq!(w.set_in(120), 120);
    assert_eq!(w.set_out(300), 300);
    assert_eq!(w.range(), 120..300);
    assert_eq!(w.selected_frames(), 180, "frame 300 itself is excluded");

    // Out cannot reach or pass the in-point, and cannot pass the end of the source.
    assert_eq!(w.set_out(120), 121);
    assert_eq!(w.set_out(0), 121);
    assert_eq!(w.set_out(600), 600, "out == total includes the last frame");
    assert_eq!(w.set_out(10_000), 600);

    // In cannot reach or pass the out-point.
    assert_eq!(w.set_out(130), 130);
    assert_eq!(w.set_in(130), 129);
    assert_eq!(w.set_in(10_000), 129);
    assert_eq!(w.set_in(0), 0);
    assert_eq!(w.range(), 0..130);

    // Marks follow the cursor.
    w.seek(45);
    let applied = w.set_in(w.cursor());
    assert_eq!((applied, w.start()), (45, 45));
}

#[test]
fn one_frame_source_keeps_a_valid_selection() {
    let mut w = window(1);
    assert_eq!(w.range(), 0..1);
    assert_eq!(w.set_in(5), 0);
    assert_eq!(w.set_out(0), 1);
    assert_eq!(w.forward(Step::SECOND), 1);
}

#[test]
fn invalid_windows_are_rejected() {
    assert!(matches!(
        FrameWindow::new(0),
        Err(MacroError::InvalidWindow(_))
    ));
    assert!(matches!(
        FrameWindow::with_fps(10, 0),
        Err(MacroError::InvalidWindow(_))
    ));
}

#[test]
fn crop_uses_the_snapshot_from_the_selected_frame_and_the_half_open_range() {
    let source = frames(600);
    let mut w = window(600);
    w.set_in(120);
    w.set_out(300);

    let rec = w.crop(&source, &snapshot_at(120), "Cut", "13.0.4").unwrap();
    assert_eq!(rec.frames, source[120..300]);
    assert_eq!(rec.frames.len(), 180);
    assert_eq!(rec.frames.first(), Some(&source[120]));
    assert_eq!(
        rec.frames.last(),
        Some(&source[299]),
        "frame 300 is not included"
    );
    // Momentum and action state come from the supplied snapshot, not the inputs.
    assert_eq!(rec.start, snapshot_at(120).state);
    assert_eq!(rec.start.speed_y, 2.5);
    assert_eq!(rec.name, "Cut");
    assert_eq!(rec.game_version, "13.0.4");
    assert_eq!(rec.placement.momentum, MomentumMode::PreserveRecorded);
    assert_eq!((rec.placement.delta_x, rec.placement.delta_y), (0.0, 0.0));
}

#[test]
fn crop_refuses_a_snapshot_for_any_other_frame() {
    let source = frames(600);
    let mut w = window(600);
    w.set_in(120);

    for stale in [0, 119, 121, 599] {
        let err = w.crop(&source, &snapshot_at(stale), "Cut", "").unwrap_err();
        assert!(
            matches!(err, MacroError::SnapshotFrame { selected: 120, snapshot } if snapshot == stale),
            "{stale}: {err}"
        );
    }

    // Moving the in-point invalidates a snapshot captured for the old one.
    let old = snapshot_at(120);
    w.set_in(180);
    assert!(matches!(
        w.crop(&source, &old, "Cut", ""),
        Err(MacroError::SnapshotFrame {
            selected: 180,
            snapshot: 120
        })
    ));
    assert!(w.crop(&source, &snapshot_at(180), "Cut", "").is_ok());
}

#[test]
fn crop_from_the_first_frame_still_needs_a_supplied_start_state() {
    let source = frames(60);
    let w = window(60);
    // Frame 0: the caller supplies a neutral start explicitly.
    let neutral = FrameSnapshot {
        frame: 0,
        state: StartState::neutral(8, 0, 1.0),
    };
    let rec = w.crop(&source, &neutral, "Whole", "").unwrap();
    assert_eq!(rec.frames, source);
    assert_eq!(rec.start, StartState::neutral(8, 0, 1.0));

    let err = w.crop(&source, &snapshot_at(5), "Whole", "").unwrap_err();
    assert!(
        matches!(
            err,
            MacroError::SnapshotFrame {
                selected: 0,
                snapshot: 5
            }
        ),
        "{err}"
    );
}

#[test]
fn crop_validates_the_source_selection_and_result() {
    let source = frames(600);
    let w = window(600);
    let err = w
        .crop(&source[..599], &snapshot_at(0), "Cut", "")
        .unwrap_err();
    assert!(
        matches!(err, MacroError::InvalidWindow(_)),
        "source of a different length: {err}"
    );

    let err = w.crop(&source, &snapshot_at(0), "  ", "").unwrap_err();
    assert!(matches!(err, MacroError::InvalidName(_)), "{err}");

    let no_fighter = FrameSnapshot {
        frame: 0,
        state: StartState::default(),
    };
    let err = w.crop(&source, &no_fighter, "Cut", "").unwrap_err();
    assert!(matches!(err, MacroError::MissingFighter), "{err}");

    let mut nan = snapshot_at(0);
    nan.state.speed_x = f32::NAN;
    let err = w.crop(&source, &nan, "Cut", "").unwrap_err();
    assert!(
        matches!(err, MacroError::NonFinite("start.speed_x")),
        "{err}"
    );
}

#[test]
fn crop_enforces_the_macro_length_limit_on_the_selection_not_the_source() {
    let source = frames(MAX_FRAMES + 600);
    let total = source.len() as u32;
    let mut w = window(total);
    let err = w.crop(&source, &snapshot_at(0), "Long", "").unwrap_err();
    assert!(matches!(err, MacroError::TooManyFrames { .. }), "{err}");

    w.set_out(MAX_FRAMES as u32);
    assert_eq!(
        w.crop(&source, &snapshot_at(0), "Max", "")
            .unwrap()
            .frames
            .len(),
        MAX_FRAMES
    );
}

#[test]
fn replay_window_to_library_round_trip_keeps_the_runtime_start_state() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let source = frames(1800);
    let mut w = window(1800);
    w.seek(60 * 5);
    w.set_in(w.cursor());
    w.forward(Step::FIVE_SECONDS);
    w.set_out(w.cursor());
    assert_eq!(w.range(), 300..600);

    let cut = w
        .crop(&source, &snapshot_at(300), "From replay", "13.0.4")
        .unwrap();
    let id = lib.save_new(&cut).unwrap();
    let loaded = lib.load(&id).unwrap();
    assert_eq!(loaded, cut);
    assert_eq!(loaded.frames, source[300..600]);
    assert_eq!(loaded.start.speed_y, 2.5);
}

#[test]
fn timestamp_reads_minutes_seconds_and_frames_in_game_time() {
    assert_eq!(timestamp(0, 60), "0:00 +0f");
    assert_eq!(timestamp(59, 60), "0:00 +59f");
    assert_eq!(timestamp(60, 60), "0:01 +0f");
    assert_eq!(timestamp(312, 60), "0:05 +12f");
    assert_eq!(timestamp(60 * 60 + 61, 60), "1:01 +1f");
    assert_eq!(timestamp(90, 30), "0:03 +0f");
    assert_eq!(timestamp(7, 0), "0:07 +0f", "fps 0 is treated as 1");
}
