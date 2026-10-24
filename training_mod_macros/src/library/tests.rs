use std::fs;

use serde_json::{json, Value};

use super::*;
use crate::{
    test_util::{frames, recording, start, TempDir},
    MomentumMode, Placement, MAX_FRAMES,
};

const _: () = assert!(SLUG_CHARS + "-9999".len() <= MAX_ID_CHARS);

fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn ids(listing: &Listing) -> Vec<&str> {
    listing.entries.iter().map(|e| e.id.as_str()).collect()
}

/// Serialized valid recording after `edit` has been applied to its JSON value.
fn edited_json(edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value = serde_json::to_value(recording("Edited")).unwrap();
    edit(&mut value);
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn missing_or_empty_directory_lists_nothing() {
    let tmp = TempDir::new();
    let absent = Library::new(tmp.path().join("absent"));
    assert_eq!(absent.list().unwrap(), Listing::default());
    assert_eq!(Library::new(tmp.path()).list().unwrap(), Listing::default());
}

#[test]
fn save_new_persists_and_reloads_through_a_fresh_library() {
    let tmp = TempDir::new();
    let dir = tmp.path().join("ultimate").join("macros");
    let mut rec = recording("Fox: Up-B drill!");
    rec.set_placement(Placement {
        delta_x: 6.5,
        delta_y: 48.0,
        momentum: MomentumMode::AtRest,
    })
    .unwrap();

    let id = Library::new(&dir).save_new(&rec).unwrap();
    assert_eq!(id, "fox-up-b-drill");

    // A new instance on the same path, as after a game restart.
    let reopened = Library::new(&dir);
    assert_eq!(reopened.load(&id).unwrap(), rec);
    let listing = reopened.list().unwrap();
    assert_eq!(
        listing.entries,
        vec![MacroEntry {
            id: id.clone(),
            name: "Fox: Up-B drill!".to_owned(),
            fighter_kind: 8,
            costume: 2,
            frame_count: 8,
        }]
    );
    assert!(listing.skipped.is_empty());
    assert_eq!(
        file_names(&dir),
        ["fox-up-b-drill.json"],
        "no temp or backup left"
    );
}

#[test]
fn same_name_saves_as_new_files_and_there_is_no_slot_limit() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let mut saved = Vec::new();
    for i in 0..25u32 {
        let mut rec = recording("Drill");
        rec.frames[0].buttons = i;
        saved.push(lib.save_new(&rec).unwrap());
    }
    assert_eq!(saved[0], "drill");
    assert_eq!(saved[1], "drill-2");
    assert_eq!(saved[24], "drill-25");
    for (i, id) in saved.iter().enumerate() {
        assert_eq!(lib.load(id).unwrap().frames[0].buttons, i as u32, "{id}");
    }
    assert_eq!(lib.list().unwrap().entries.len(), 25);
}

#[test]
fn listing_is_sorted_by_name_ignoring_case_then_id() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    for name in ["banana", "Apple", "cherry", "apple"] {
        lib.save_new(&recording(name)).unwrap();
    }
    let listing = lib.list().unwrap();
    assert_eq!(ids(&listing), ["apple", "apple-2", "banana", "cherry"]);
}

#[test]
fn overwrite_replaces_the_target_keeps_its_id_and_leaves_others_alone() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let alpha = lib.save_new(&recording("Alpha")).unwrap();
    let beta = lib.save_new(&recording("Beta")).unwrap();

    // Metadata-only edit: placement and name change, the frames do not.
    let mut edited = lib.load(&alpha).unwrap();
    let frames_before = edited.frames.clone();
    edited
        .set_placement(Placement {
            delta_x: 3.0,
            delta_y: 40.0,
            momentum: MomentumMode::AtRest,
        })
        .unwrap();
    edited.set_name("Alpha v2").unwrap();
    lib.overwrite(&alpha, &edited).unwrap();

    let reloaded = Library::new(tmp.path()).load(&alpha).unwrap();
    assert_eq!(reloaded, edited);
    assert_eq!(reloaded.frames, frames_before);
    assert_eq!(lib.load(&beta).unwrap(), recording("Beta"));

    let listing = lib.list().unwrap();
    assert_eq!(
        ids(&listing),
        ["alpha", "beta"],
        "the id is stable across a rename"
    );
    assert_eq!(listing.entries[0].name, "Alpha v2");
    assert_eq!(file_names(tmp.path()), ["alpha.json", "beta.json"]);
}

#[test]
fn a_placement_edit_can_be_saved_as_new_while_the_original_keeps_its_placement() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let original_id = lib.save_new(&recording("Drill")).unwrap();

    let mut edited = lib.load(&original_id).unwrap();
    edited
        .set_placement(Placement {
            delta_x: -8.0,
            delta_y: 60.0,
            momentum: MomentumMode::PreserveRecorded,
        })
        .unwrap();
    let copy_id = lib.save_new(&edited).unwrap();

    assert_ne!(copy_id, original_id);
    assert_eq!(
        lib.load(&original_id).unwrap().placement,
        Placement::default()
    );
    let copy = lib.load(&copy_id).unwrap();
    assert_eq!(copy.placement.delta_y, 60.0);
    assert_eq!(copy.frames, recording("Drill").frames);
}

#[test]
fn overwrite_requires_an_existing_id_and_creates_nothing() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let err = lib.overwrite("ghost", &recording("Ghost")).unwrap_err();
    assert!(
        matches!(err, MacroError::NotFound(ref id) if id == "ghost"),
        "{err}"
    );
    assert!(file_names(tmp.path()).is_empty());
}

#[test]
fn overwrite_can_repair_a_corrupt_file() {
    let tmp = TempDir::new();
    fs::write(tmp.path().join("broken.json"), "{ not json").unwrap();
    let lib = Library::new(tmp.path());
    let fixed = recording("Fixed");
    lib.overwrite("broken", &fixed).unwrap();
    assert_eq!(lib.load("broken").unwrap(), fixed);
}

#[test]
fn invalid_input_never_touches_an_existing_file_or_creates_one() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let id = lib.save_new(&recording("Alpha")).unwrap();
    let path = tmp.path().join(format!("{id}.json"));
    let before = fs::read(&path).unwrap();

    let mut nan = recording("Alpha");
    nan.placement.delta_x = f32::NAN;
    assert!(matches!(
        lib.overwrite(&id, &nan),
        Err(MacroError::NonFinite(_))
    ));
    let mut empty = recording("Alpha");
    empty.frames.clear();
    assert!(matches!(
        lib.overwrite(&id, &empty),
        Err(MacroError::EmptyFrames)
    ));
    let mut future = recording("Alpha");
    future.schema_version = 9;
    assert!(matches!(
        lib.overwrite(&id, &future),
        Err(MacroError::UnsupportedVersion { found: 9 })
    ));
    let mut huge = recording("Alpha");
    huge.frames = frames(MAX_FRAMES + 1);
    assert!(matches!(
        lib.overwrite(&id, &huge),
        Err(MacroError::TooManyFrames { .. })
    ));

    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(file_names(tmp.path()), [format!("{id}.json")]);

    // An invalid save_new does not even create the library directory.
    let fresh_dir = tmp.path().join("fresh");
    assert!(Library::new(&fresh_dir).save_new(&nan).is_err());
    assert!(!fresh_dir.exists());
}

#[test]
fn a_failed_write_keeps_the_previous_good_file() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let original = recording("Alpha");
    let id = lib.save_new(&original).unwrap();
    let path = tmp.path().join(format!("{id}.json"));
    let before = fs::read(&path).unwrap();

    // Occupy the temp path with a directory so the new data cannot be written.
    fs::create_dir(tmp.path().join(format!("{id}.json.tmp"))).unwrap();
    let mut changed = recording("Alpha");
    changed.frames.truncate(2);
    assert!(matches!(
        lib.overwrite(&id, &changed),
        Err(MacroError::Io(_))
    ));

    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(lib.load(&id).unwrap(), original);
}

#[test]
fn corrupt_files_are_reported_and_never_lost_while_good_ones_still_list() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let good_id = lib.save_new(&recording("Good")).unwrap();
    let good_bytes = fs::read(tmp.path().join("good.json")).unwrap();

    let truncated = &good_bytes[..good_bytes.len() / 2];
    let bad: Vec<(&str, Vec<u8>)> = vec![
        ("truncated.json", truncated.to_vec()),
        ("garbage.json", b"not json at all".to_vec()),
        ("empty.json", Vec::new()),
        (
            "future.json",
            edited_json(|v| v["schema_version"] = json!(3)),
        ),
        (
            "overflow.json",
            edited_json(|v| v["start"]["facing"] = json!(1.0e39)),
        ),
        (
            "nullfloat.json",
            edited_json(|v| v["start"]["speed_y"] = Value::Null),
        ),
        ("noframes.json", edited_json(|v| v["frames"] = json!([]))),
        ("noname.json", edited_json(|v| v["name"] = json!("  "))),
        ("Bad Name.json", good_bytes.clone()),
        ("big.json", vec![b' '; MAX_FILE_BYTES as usize + 1]),
    ];
    for (name, bytes) in &bad {
        fs::write(tmp.path().join(name), bytes).unwrap();
    }
    // Not macro files at all: ignored, not reported.
    fs::write(tmp.path().join("notes.txt"), "hello").unwrap();
    fs::write(tmp.path().join("stale.json.tmp"), "half a wri").unwrap();
    fs::create_dir(tmp.path().join("folder.json")).unwrap();

    let listing = lib.list().unwrap();
    assert_eq!(ids(&listing), [good_id.as_str()]);

    let mut skipped: Vec<&str> = listing.skipped.iter().map(|s| s.file.as_str()).collect();
    skipped.sort_unstable();
    let mut expected: Vec<&str> = bad.iter().map(|(n, _)| *n).collect();
    expected.sort_unstable();
    assert_eq!(skipped, expected);

    let reason = |file: &str| {
        listing
            .skipped
            .iter()
            .find(|s| s.file == file)
            .unwrap()
            .reason
            .as_str()
    };
    assert!(
        reason("future.json").contains("unsupported schema version 3"),
        "{}",
        reason("future.json")
    );
    assert!(
        // float_roundtrip rejects f32 overflow while parsing; the older parser
        // reached our finite-value validator. Both must preserve the bad file.
        reason("overflow.json").contains("finite") || reason("overflow.json").contains("number out of range"),
        "{}",
        reason("overflow.json")
    );
    assert!(
        reason("noframes.json").contains("no frames"),
        "{}",
        reason("noframes.json")
    );
    assert!(
        reason("big.json").contains("bytes"),
        "{}",
        reason("big.json")
    );
    assert!(
        reason("Bad Name.json").contains("invalid macro id"),
        "{}",
        reason("Bad Name.json")
    );
    assert!(
        reason("garbage.json").contains("malformed"),
        "{}",
        reason("garbage.json")
    );

    // Reporting must not modify, move or delete anything.
    for (name, bytes) in &bad {
        assert_eq!(&fs::read(tmp.path().join(name)).unwrap(), bytes, "{name}");
    }
    assert_eq!(fs::read(tmp.path().join("good.json")).unwrap(), good_bytes);
    assert_eq!(lib.load(&good_id).unwrap(), recording("Good"));
}

#[test]
fn load_reports_the_specific_failure() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    fs::write(
        tmp.path().join("future.json"),
        edited_json(|v| v["schema_version"] = json!(3)),
    )
    .unwrap();
    // A version-3 file whose layout this crate cannot parse is still a version error.
    fs::write(
        tmp.path().join("shape.json"),
        br#"{"schema_version":3,"frames":"packed"}"#,
    )
    .unwrap();
    fs::write(
        tmp.path().join("big.json"),
        vec![b' '; MAX_FILE_BYTES as usize + 1],
    )
    .unwrap();
    fs::write(tmp.path().join("garbage.json"), "[1,2").unwrap();

    assert!(matches!(
        lib.load("future"),
        Err(MacroError::UnsupportedVersion { found: 3 })
    ));
    assert!(matches!(
        lib.load("shape"),
        Err(MacroError::UnsupportedVersion { found: 3 })
    ));
    assert!(matches!(
        lib.load("big"),
        Err(MacroError::FileTooLarge { .. })
    ));
    assert!(matches!(lib.load("garbage"), Err(MacroError::Parse(_))));
    assert!(matches!(lib.load("nothing"), Err(MacroError::NotFound(ref id)) if id == "nothing"));
}

#[test]
fn unsafe_ids_are_refused_and_cannot_escape_the_directory() {
    let tmp = TempDir::new();
    let dir = tmp.path().join("lib");
    fs::create_dir(&dir).unwrap();
    let lib = Library::new(&dir);
    // A real macro in the parent directory that a traversal would reach.
    Library::new(tmp.path())
        .save_new(&recording("Outside"))
        .unwrap();
    let rec = recording("Mine");

    let too_long = "a".repeat(MAX_ID_CHARS + 1);
    for bad in [
        "../outside",
        "..",
        ".",
        "",
        "a/b",
        "a\\b",
        "UPPER",
        "has space",
        "x.json",
        ".hidden",
        "é",
        "nul\0",
        too_long.as_str(),
    ] {
        assert!(
            matches!(lib.load(bad), Err(MacroError::InvalidId(_))),
            "load {bad:?}"
        );
        assert!(
            matches!(lib.overwrite(bad, &rec), Err(MacroError::InvalidId(_))),
            "overwrite {bad:?}"
        );
    }
    assert!(matches!(lib.load("outside"), Err(MacroError::NotFound(_))));
    assert!(file_names(&dir).is_empty());
    assert_eq!(
        Library::new(tmp.path()).load("outside").unwrap(),
        recording("Outside"),
        "the file outside the library is untouched"
    );

    let longest = "a".repeat(MAX_ID_CHARS);
    assert!(check_id(&longest).is_ok());
    assert!(check_id("a-b_c9").is_ok());
}

#[test]
fn ids_are_lowercase_ascii_slugs_that_fit_the_id_limit() {
    for (name, expected) in [
        ("Fox: Up-B drill!", "fox-up-b-drill"),
        ("  --Hello__World--  ", "hello-world"),
        ("A", "a"),
        ("日本語", "macro"),
        ("!!!", "macro"),
    ] {
        assert_eq!(slug(name), expected, "{name:?}");
    }
    for name in ["a".repeat(100), "word ".repeat(30), "x-".repeat(60)] {
        let id = slug(&name);
        assert!(id.len() <= SLUG_CHARS, "{id}");
        assert!(!id.ends_with('-') && check_id(&id).is_ok(), "{id}");
    }
}

#[test]
fn save_new_never_reuses_an_id_even_when_the_existing_file_is_corrupt() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    fs::write(tmp.path().join("drill.json"), "garbage").unwrap();
    fs::write(tmp.path().join("solo.json.bak"), "recoverable backup").unwrap();

    assert_eq!(lib.save_new(&recording("Drill")).unwrap(), "drill-2");
    assert_eq!(lib.save_new(&recording("Solo")).unwrap(), "solo-2");
    assert_eq!(
        fs::read_to_string(tmp.path().join("drill.json")).unwrap(),
        "garbage"
    );
    assert_eq!(
        fs::read_to_string(tmp.path().join("solo.json.bak")).unwrap(),
        "recoverable backup"
    );
}

#[test]
fn names_without_ascii_still_get_unique_ids_and_keep_their_display_name() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    assert_eq!(lib.save_new(&recording("日本語")).unwrap(), "macro");
    assert_eq!(lib.save_new(&recording("日本語")).unwrap(), "macro-2");
    let listing = lib.list().unwrap();
    assert!(listing.entries.iter().all(|e| e.name == "日本語"));
}

#[test]
fn backup_swap_replaces_the_file_and_cleans_up() {
    let tmp = TempDir::new();
    let dest = tmp.path().join("x.json");
    let tmp_file = tmp.path().join("x.json.tmp");
    fs::write(&dest, "old").unwrap();
    fs::write(&tmp_file, "new").unwrap();

    replace_via_backup(&tmp_file, &dest).unwrap();
    assert_eq!(fs::read_to_string(&dest).unwrap(), "new");
    assert_eq!(file_names(tmp.path()), ["x.json"]);
}

#[test]
fn backup_swap_restores_the_old_file_when_the_swap_fails() {
    let tmp = TempDir::new();
    let dest = tmp.path().join("x.json");
    fs::write(&dest, "old").unwrap();

    // The temp file is missing, so the second rename must fail.
    assert!(replace_via_backup(&tmp.path().join("x.json.tmp"), &dest).is_err());
    assert_eq!(fs::read_to_string(&dest).unwrap(), "old");
    assert_eq!(file_names(tmp.path()), ["x.json"]);
}

#[test]
fn list_restores_an_orphaned_backup_but_leaves_a_stale_one_alone() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let good = serde_json::to_vec(&recording("Lost and found")).unwrap();
    fs::write(tmp.path().join("lost.json.bak"), &good).unwrap();

    let kept = lib.save_new(&recording("Kept")).unwrap();
    fs::write(tmp.path().join(format!("{kept}.json.bak")), &good).unwrap();

    let listing = lib.list().unwrap();
    assert_eq!(ids(&listing), [kept.as_str(), "lost"]);
    assert!(tmp.path().join("lost.json").exists());
    assert!(!tmp.path().join("lost.json.bak").exists());
    assert!(
        tmp.path().join(format!("{kept}.json.bak")).exists(),
        "a backup next to a live file is not touched"
    );
    assert_eq!(lib.load(&kept).unwrap(), recording("Kept"));
}

#[test]
fn start_state_survives_persistence_exactly() {
    let tmp = TempDir::new();
    let lib = Library::new(tmp.path());
    let id = lib.save_new(&recording("State")).unwrap();
    assert_eq!(lib.load(&id).unwrap().start, start());
}

#[test]
fn exact_mii_library_restart_and_save_as_preserve_configuration() {
    let tmp = TempDir::new();
    let configuration = crate::test_util::exact_mii();
    let mut rec = MacroRecording::new("Captured Brawler", "13.0.5", crate::StartState::neutral(0x48, 0, 1.0), frames(2)).unwrap();
    rec.set_mii(configuration).unwrap();
    let id = Library::new(tmp.path()).save_new(&rec).unwrap();
    let reopened = Library::new(tmp.path());
    assert_eq!(reopened.load(&id).unwrap(), rec);
    assert!(reopened.list().unwrap().skipped.is_empty());
    let mut copy = reopened.load(&id).unwrap();
    copy.set_name("Brawler placed").unwrap();
    copy.set_placement(Placement {delta_x: 4.0, ..Placement::default()}).unwrap();
    let copy_id = reopened.save_new(&copy).unwrap();
    assert_ne!(id, copy_id);
    let final_copy = Library::new(tmp.path()).load(&copy_id).unwrap();
    assert_eq!(final_copy, copy);
    assert_eq!(final_copy.playback_target().unwrap().configuration(), Some(configuration));
    assert_eq!(reopened.load(&id).unwrap(), rec);
}
