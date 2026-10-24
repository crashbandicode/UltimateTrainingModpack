//! Shared fixtures for the unit tests.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use crate::{InputFrame, MacroRecording, StartState};

/// A unique directory under the system temp dir, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "training_mod_macros-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// `count` distinct frames, so any reordering or truncation shows up.
pub fn frames(count: usize) -> Vec<InputFrame> {
    (0..count)
        .map(|i| InputFrame {
            buttons: i as u32 * 7,
            lstick_x: (i % 127) as i8,
            lstick_y: -((i % 127) as i8),
            rstick_x: i8::MIN,
            rstick_y: i8::MAX,
        })
        .collect()
}

/// A mid-air, moving, left-facing start so no field is accidentally default.
pub fn start() -> StartState {
    StartState {
        fighter_kind: 8,
        costume: 2,
        facing: -1.0,
        situation: 3,
        status: 17,
        speed_x: 1.25,
        speed_y: -0.5,
    }
}

pub fn recording(name: &str) -> MacroRecording {
    MacroRecording::new(name, "13.0.4", start(), frames(8)).unwrap()
}

pub fn exact_mii() -> training_mod_mii::lifetime::Configuration {
    use training_mod_mii::inventory::{classify, MiiClass, RawSlot, TaggedId, CHARINFO_LEN, TAG_CHARA, TAG_HAT, TAG_OUTFIT};
    let saved = classify(&RawSlot {
        index: 0, slot_id: 13231, name_len: 1,
        name_units: [65, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        charinfo: [7; CHARINFO_LEN], colour: 4, voice: 37, specials: [1, 0, 0, 0],
        tag_outfit: TaggedId((u64::from(TAG_OUTFIT) << 56) | (1 << 40) | 0x1b_fef7_a5f7),
        tag_hat: TaggedId((u64::from(TAG_HAT) << 56) | 0x0f_a9ef_a5ea),
        tag_chara: TaggedId((u64::from(TAG_CHARA) << 56) | MiiClass::Brawler.chara_hash40()),
    }).unwrap();
    training_mod_mii::lifetime::Configuration::from_saved(&saved).unwrap()
}
