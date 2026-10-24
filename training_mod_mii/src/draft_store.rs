//! Bounded, mod-owned preset persistence. Native Mii saves are never a destination.
//! One menu/filesystem owner must serialize access to this directory.
use std::{fs, io::{self, Read, Write}, path::{Path, PathBuf}};
use crate::{draft::{Draft, Library, Saved}, inventory::Snapshot};

pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
const FILE: &str = "mii-presets.json";

pub struct Store {
    dir: PathBuf,
    library: Library,
}

impl Store {
    /// A missing file is an empty library. Invalid/oversized files are errors and
    /// remain untouched. Recover a validated backup only if its destination is absent.
    pub fn open(mod_directory: impl Into<PathBuf>) -> io::Result<Self> {
        let dir = mod_directory.into();
        let dest = dir.join(FILE);
        let library = match read_library(&dest) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let backup = dir.join(format!("{FILE}.bak"));
                match read_library(&backup) {
                    Ok(value) => { fs::rename(&backup, &dest)?; value }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => Library::default(),
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };
        Ok(Self { dir, library })
    }

    pub fn library(&self) -> &Library { &self.library }

    /// Commit on disk before publishing the new in-memory library. Duplicate
    /// configurations preserve their first name and do not write any file.
    pub fn save(&mut self, draft: &Draft, name: &str, native: &Snapshot) -> io::Result<Saved> {
        let mut next = self.library.clone();
        let outcome = next.save(draft, name, native)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.commit(next, outcome)
    }

    pub fn import(&mut self, configuration: crate::lifetime::Configuration, name: &str, native: &Snapshot) -> io::Result<Saved> {
        let mut next = self.library.clone();
        let outcome = next.import(configuration, name, native)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        self.commit(next, outcome)
    }

    fn commit(&mut self, next: Library, outcome: Saved) -> io::Result<Saved> {
        if !matches!(outcome, Saved::Created(_)) { return Ok(outcome); }
        let bytes = serde_json::to_vec(&next).map_err(invalid_data)?;
        if bytes.len() as u64 > MAX_FILE_BYTES { return Err(invalid_data("Mii preset file is too large")); }
        fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("{FILE}.tmp"));
        let dest = self.dir.join(FILE);
        // This scratch file belongs only to this mod store. A previous interrupted
        // scratch write was never committed and is not a valid library to recover.
        let mut file = fs::File::create(&tmp)?;
        let written = file.write_all(&bytes).and_then(|()| file.sync_all());
        drop(file);
        let result = written.and_then(|()| replace_file(&tmp, &dest));
        if result.is_err() { let _ = fs::remove_file(&tmp); }
        result?;
        self.library = next;
        Ok(outcome)
    }
}

fn invalid_data(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

fn read_library(path: &Path) -> io::Result<Library> {
    // Pinned Skyline open() loses NotFound; metadata() preserves its typed result.
    if fs::metadata(path)?.len() > MAX_FILE_BYTES { return Err(invalid_data("Mii preset file is too large")); }
    let file = fs::File::open(path)?;
    // Bound the read itself as well as metadata; file growth cannot bypass the cap.
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES { return Err(invalid_data("Mii preset file is too large")); }
    serde_json::from_slice(&bytes).map_err(invalid_data)
}

/// Same rename/backup strategy used by the macro library for Horizon filesystems
/// whose rename cannot replace a file. An interrupted fallback keeps the old .bak.
fn replace_file(tmp: &Path, dest: &Path) -> io::Result<()> {
    let first = match fs::rename(tmp, dest) { Ok(()) => return Ok(()), Err(error) => error };
    if !dest.try_exists()? { return Err(first); }
    replace_via_backup(tmp, dest)
}

fn replace_via_backup(tmp: &Path, dest: &Path) -> io::Result<()> {
    let backup = dest.with_file_name(format!("{FILE}.bak"));
    match fs::remove_file(&backup) {
        Ok(()) => {},
        Err(error) if error.kind() == io::ErrorKind::NotFound => {},
        Err(error) => return Err(error),
    }
    fs::rename(dest, &backup)?;
    if let Err(error) = fs::rename(tmp, dest) {
        // If restoration itself fails, leave the old backup for validated open().
        let _ = fs::rename(&backup, dest);
        return Err(error);
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{classify, MiiClass, RawSlot, TaggedId, CHARINFO_LEN, TAG_CHARA, TAG_HAT, TAG_OUTFIT};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!("mii-store-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }

    fn draft(moves: [u8; 4]) -> Draft {
        let template = classify(&RawSlot { index: 0, slot_id: 2622, name_len: 1,
            name_units: [65, 0, 0, 0, 0, 0, 0, 0, 0, 0], colour: 1, voice: 37,
            specials: [0; 4], charinfo: [7; CHARINFO_LEN],
            tag_outfit: TaggedId((u64::from(TAG_OUTFIT) << 56) | (1 << 40) | 0x1bfef7a5f7),
            tag_hat: TaggedId((u64::from(TAG_HAT) << 56) | 0x0fa9efa5ea),
            tag_chara: TaggedId((u64::from(TAG_CHARA) << 56) | MiiClass::Swordfighter.chara_hash40()),
        }).unwrap();
        let mut draft = Draft::from_template(&template).unwrap();
        draft.set_specials(moves).unwrap();
        draft
    }

    #[test]
    fn failed_write_preserves_committed_disk_and_memory() {
        let dir = Temp::new();
        let mut store = Store::open(&dir.0).unwrap();
        store.save(&draft([1; 4]), "First", &Snapshot::default()).unwrap();
        let before = fs::read(dir.0.join(FILE)).unwrap();
        let memory = store.library().clone();
        fs::create_dir(dir.0.join(format!("{FILE}.tmp"))).unwrap();
        assert!(store.save(&draft([2; 4]), "Second", &Snapshot::default()).is_err());
        assert_eq!(fs::read(dir.0.join(FILE)).unwrap(), before);
        assert_eq!(store.library(), &memory);
        // Duplicate succeeds without a write even while the scratch destination is blocked.
        assert!(matches!(store.save(&draft([1; 4]), "Rename", &Snapshot::default()).unwrap(), Saved::ExistingPreset(_)));
        assert_eq!(store.library().presets()[0].name(), "First");
    }

    #[test]
    fn invalid_and_oversized_libraries_are_not_replaced() {
        let dir = Temp::new();
        let dest = dir.0.join(FILE);
        fs::write(&dest, b"{invalid").unwrap();
        assert!(Store::open(&dir.0).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"{invalid");
        let file = fs::File::create(&dest).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        assert!(Store::open(&dir.0).is_err());
        assert_eq!(fs::metadata(dest).unwrap().len(), MAX_FILE_BYTES + 1);
    }

    #[test]
    fn validated_backup_recovers_an_interrupted_swap() {
        let dir = Temp::new();
        let mut store = Store::open(&dir.0).unwrap();
        store.save(&draft([0, 2, 1, 2]), "Kept", &Snapshot::default()).unwrap();
        let before = store.library().clone();
        let dest = dir.0.join(FILE);
        let backup = dir.0.join(format!("{FILE}.bak"));
        fs::rename(&dest, &backup).unwrap();
        assert_eq!(Store::open(&dir.0).unwrap().library(), &before);
        assert!(dest.exists());
        assert!(!backup.exists());
        fs::rename(&dest, &backup).unwrap();
        fs::write(&backup, b"bad backup").unwrap();
        assert!(Store::open(&dir.0).is_err());
        assert!(!dest.exists());
        assert_eq!(fs::read(backup).unwrap(), b"bad backup");
    }

    #[test]
    fn fallback_replaces_or_restores_the_existing_file() {
        let dir = Temp::new();
        let dest = dir.0.join(FILE);
        let tmp = dir.0.join(format!("{FILE}.tmp"));
        fs::write(&dest, b"old").unwrap();
        assert!(replace_via_backup(&tmp, &dest).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"old");
        fs::write(&tmp, b"new").unwrap();
        replace_via_backup(&tmp, &dest).unwrap();
        assert_eq!(fs::read(dest).unwrap(), b"new");
    }

    #[test]
    fn preset_continues_in_a_fresh_process() {
        const PHASE: &str = "TRAINING_MII_STORE_TEST_PHASE";
        const DIRECTORY: &str = "TRAINING_MII_STORE_TEST_DIRECTORY";
        if let Ok(phase) = std::env::var(PHASE) {
            let dir = PathBuf::from(std::env::var_os(DIRECTORY).unwrap());
            let mut store = Store::open(dir).unwrap();
            if phase == "save" {
                store.save(&draft([2, 1, 0, 2]), "Persisted", &Snapshot::default()).unwrap();
            } else {
                assert_eq!(phase, "continue");
                assert_eq!(store.library().presets().len(), 1);
                assert_eq!(store.library().presets()[0].configuration(), draft([2, 1, 0, 2]).configuration().unwrap());
                assert!(matches!(store.save(&draft([2, 1, 0, 2]), "Different name", &Snapshot::default()).unwrap(), Saved::ExistingPreset(_)));
                store.save(&draft([1, 0, 2, 1]), "Next", &Snapshot::default()).unwrap();
            }
            println!("mii-store phase={phase} pid={} completed", std::process::id());
            return;
        }
        let dir = Temp::new();
        for phase in ["save", "continue"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "draft_store::tests::preset_continues_in_a_fresh_process", "--nocapture"])
                .env(PHASE, phase).env(DIRECTORY, &dir.0).output().unwrap();
            assert!(output.status.success(), "{phase}: {}", String::from_utf8_lossy(&output.stderr));
            let stdout = String::from_utf8(output.stdout).unwrap();
            assert!(stdout.contains(&format!("mii-store phase={phase} pid=")));
            // Keep child identity/exit evidence without counting nested harness summaries.
            println!("{} exit=0", stdout.lines().find(|line| line.starts_with("mii-store phase=")).unwrap());
        }
        let continued = Store::open(&dir.0).unwrap();
        assert_eq!(continued.library().presets().len(), 2);
        assert_eq!(continued.library().presets()[0].name(), "Persisted");
        assert_eq!(continued.library().presets()[1].configuration(), draft([1, 0, 2, 1]).configuration().unwrap());
    }
}
