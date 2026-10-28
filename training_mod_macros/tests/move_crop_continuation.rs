//! Save a non-neutral source crop, exit, then reload its neutral input restart in
//! another process. No game assets or proprietary replay bytes are fixtures.
use std::{fs, path::PathBuf, process::Command};
use training_mod_macros::*;

#[test]
fn snapped_crop_continues_after_the_saving_process_exits() {
    let root = std::env::temp_dir().join(format!("move-crop-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    let _cleanup = Cleanup(root.clone());
    let run = |phase| {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "move_crop_child", "--nocapture"])
            .env("MOVE_CROP_PHASE", phase).env("MOVE_CROP_DIR", &root).output().unwrap();
        assert!(child.status.success(), "{}", String::from_utf8_lossy(&child.stdout));
    };
    run("save"); // output() waits for successful saver exit before next launch.
    assert!(root.join("id").is_file());
    run("continue");
    assert_eq!(fs::read_to_string(root.join("continued")).unwrap(), "6 inputs; start=0; first=-127");
}

#[test]
fn move_crop_child() {
    let Ok(phase) = std::env::var("MOVE_CROP_PHASE") else { return };
    let root = PathBuf::from(std::env::var_os("MOVE_CROP_DIR").unwrap());
    let library = Library::new(root.join("library"));
    match phase.as_str() {
        "save" => {
            let statuses = [486, 486, 6, 7, 45, 46, 47];
            let track = ReplayTrack { entry: 1, quantized_analog: false,
                frames: statuses.into_iter().enumerate().map(|(i, status)| ReplayFrame {
                    input: InputFrame { buttons: i as u32, lstick_x: -127, ..Default::default() },
                    start: StartState { status, speed_x: 4.0, ..StartState::neutral(18, 0, 1.0) },
                    x: 12.0, y: 0.0,
                }).collect() };
            let rules = MoveStartRules { families: vec![MoveFamily { fighter: None,
                statuses: vec![45, 46, 47], buttons: 0 }], setup: vec![6, 7], ground: 0, air: 2,
                wait: 0, fall: 14, trigger_buttons: 0 };
            let snap = rules.snap(&track, 6).unwrap();
            assert_eq!(snap.frame, 1);
            assert_eq!(track.frames[1].start.status, 486);
            let mut window = track.window().unwrap(); window.set_in(snap.frame);
            let recording = track.export_move(&window, "middle move", "13.0.5", &snap).unwrap();
            assert_eq!(recording.start.status, 0);
            assert_eq!(recording.start.speed_x, 0.0);
            let id = library.save_new(&recording).unwrap();
            fs::write(root.join("id"), id).unwrap();
        }
        "continue" => {
            let id = fs::read_to_string(root.join("id")).unwrap();
            let loaded = library.load(&id).unwrap();
            assert_eq!(loaded.start.status, 0);
            assert_eq!(loaded.start.speed_x, 0.0);
            assert_eq!(loaded.frames.iter().map(|f| f.buttons).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
            assert_eq!(loaded.frames[0].lstick_x, -127);
            fs::write(root.join("continued"), "6 inputs; start=0; first=-127").unwrap();
        }
        _ => panic!("unknown child phase"),
    }
}
