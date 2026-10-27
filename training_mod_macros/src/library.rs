//! Directory-backed record library: one compact JSON file per entry, named
//! `<id>.json`, in a caller-chosen directory. There is no entry-count limit.
//!
//! [`Store`] is generic over the stored [`Record`]. [`Library`] is its
//! named-macro instantiation and keeps the original API; the portable practice
//! setup in `training_mod_consts` is a second instantiation that reuses the id,
//! size, version and atomic-write rules below instead of copying them.
//!
//! * Ids are stable and path-safe (`[a-z0-9_-]`, at most [`MAX_ID_CHARS`]). The
//!   id is the file stem; it never changes when the record is overwritten.
//! * Updates write `<id>.json.tmp`, sync it, then rename it over the target, so
//!   a reader or an interrupted write never sees a partial file. Where rename
//!   cannot replace an existing file (FAT on Horizon is the concern), the old
//!   file is moved to `<id>.json.bak` first and put back if the swap fails.
//! * `list` never deletes or rewrites anything except restoring an orphaned
//!   `.bak` (left only by a crash inside that swap).

use std::{
    ffi::OsString,
    fmt, fs,
    io::{self, Write},
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::recording::{MacroError, MacroRecording, MII_SCHEMA_VERSION, SCHEMA_VERSION};

/// Longest id accepted, in characters.
pub const MAX_ID_CHARS: usize = 48;
/// Largest macro file read: 8 MiB. A worst-case `MAX_FRAMES` macro is about 3 MB as
/// written here and about 5 MB pretty-printed, so hand-edited files still load
/// while anything far larger is rejected before it is read. Other record types
/// set their own bound through [`Record::MAX_FILE_BYTES`].
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

const SLUG_CHARS: usize = 40;
const MAX_SUFFIX: u32 = 9_999;

/// A value type a [`Store`] keeps as one JSON file per entry.
///
/// The store owns ids, atomic replacement, size bounds and the
/// version-versus-corruption distinction. The record owns its own validation,
/// including rejecting a `schema_version` outside [`Record::SCHEMA_VERSIONS`]:
/// the store's header check only classifies files that do not parse at all.
pub trait Record: Serialize + DeserializeOwned {
    /// Error reported by a store of this record. The store's own failures
    /// (`MacroError`: I/O, id, size, parse, version) must convert into it.
    type Error: From<MacroError> + fmt::Display;
    /// Listing row built from one validated record.
    type Summary;
    /// Schema versions this reader accepts. A file whose `schema_version`
    /// header is readable but outside this set is reported as
    /// `UnsupportedVersion` rather than as corruption.
    const SCHEMA_VERSIONS: &'static [u32];
    /// Largest file `load` and `list` will read.
    const MAX_FILE_BYTES: u64;
    /// Everything the store requires before it writes or accepts a file.
    fn validate(&self) -> Result<(), Self::Error>;
    /// Display name: its slug seeds new ids and it orders listings.
    fn name(&self) -> &str;
    /// Listing row for the record stored under `id`.
    fn summary(&self, id: &str) -> Self::Summary;
}

/// Summary of one loadable macro, enough to fill a selection dropdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroEntry {
    pub id: String,
    pub name: String,
    pub fighter_kind: i32,
    pub costume: u32,
    pub frame_count: usize,
}

/// A `.json` file that could not be used, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedFile {
    pub file: String,
    pub reason: String,
}

/// Result of scanning a store: good entries plus the files that were skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listing<S = MacroEntry> {
    /// Sorted by record name (case-insensitive), then id.
    pub entries: Vec<S>,
    pub skipped: Vec<SkippedFile>,
}

// Not derived: a derive would demand `S: Default`, which summaries do not have.
#[allow(clippy::derivable_impls)]
impl<S> Default for Listing<S> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            skipped: Vec::new(),
        }
    }
}

/// A record store rooted at a caller-supplied directory.
pub struct Store<R> {
    dir: PathBuf,
    record: PhantomData<fn() -> R>,
}

/// The named-macro library: the original API, now on the generic store.
pub type Library = Store<MacroRecording>;

impl<R> Clone for Store<R> {
    fn clone(&self) -> Self {
        Self {
            dir: self.dir.clone(),
            record: PhantomData,
        }
    }
}

impl<R> fmt::Debug for Store<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store").field("dir", &self.dir).finish()
    }
}

impl Record for MacroRecording {
    type Error = MacroError;
    type Summary = MacroEntry;
    const SCHEMA_VERSIONS: &'static [u32] = &[SCHEMA_VERSION, MII_SCHEMA_VERSION];
    const MAX_FILE_BYTES: u64 = MAX_FILE_BYTES;

    fn validate(&self) -> Result<(), MacroError> {
        MacroRecording::validate(self)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn summary(&self, id: &str) -> MacroEntry {
        MacroEntry {
            id: id.to_owned(),
            name: self.name.clone(),
            fighter_kind: self.start.fighter_kind,
            costume: self.start.costume,
            frame_count: self.frames.len(),
        }
    }
}

impl<R: Record> Store<R> {
    /// Does no I/O; the directory is created by the first save.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            record: PhantomData,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Lists every valid record. A missing directory is an empty store.
    /// Corrupt or invalid files are reported in `skipped` and left untouched.
    /// Files are decoded one at a time; only each summary is retained.
    pub fn list(&self) -> Result<Listing<R::Summary>, R::Error> {
        let mut listing = Listing::default();
        let read_dir = match fs::read_dir(&self.dir) {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(listing),
            Err(err) => return Err(lift::<R>(err.into())),
        };
        let mut names = Vec::new();
        for item in read_dir {
            match item {
                Ok(item) if item.path().is_file() => {
                    names.push(item.file_name().to_string_lossy().into_owned())
                }
                Ok(_) => {}
                Err(err) => listing.skipped.push(SkippedFile {
                    file: String::new(),
                    reason: format!("unreadable directory entry: {err}"),
                }),
            }
        }
        self.restore_orphaned_backups(&mut names);
        names.sort();
        let mut entries: Vec<(String, String, R::Summary)> = Vec::new();
        for name in names {
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            let result = check_id(id)
                .and_then(|()| read_bounded(&self.file(id, "json"), R::MAX_FILE_BYTES))
                .map_err(lift::<R>)
                .and_then(|data| decode::<R>(&data));
            match result {
                Ok(rec) => entries.push((rec.name().to_lowercase(), id.to_owned(), rec.summary(id))),
                Err(err) => listing.skipped.push(SkippedFile {
                    file: name.clone(),
                    reason: err.to_string(),
                }),
            }
        }
        entries.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        listing.entries = entries.into_iter().map(|(_, _, summary)| summary).collect();
        Ok(listing)
    }

    /// Loads and fully validates the record with this id.
    pub fn load(&self, id: &str) -> Result<R, R::Error> {
        check_id(id).map_err(lift::<R>)?;
        let data = read_bounded(&self.file(id, "json"), R::MAX_FILE_BYTES)
            .map_err(|err| match err {
                MacroError::Io(inner) if inner.kind() == io::ErrorKind::NotFound => {
                    MacroError::NotFound(id.to_owned())
                }
                other => other,
            })
            .map_err(lift::<R>)?;
        decode::<R>(&data)
    }

    /// Saves as a brand-new file and returns its id. The id comes from the
    /// name (`my-drill`, then `my-drill-2`, ...) and never matches an existing
    /// file, good or corrupt, so nothing already on disk can be replaced.
    pub fn save_new(&self, rec: &R) -> Result<String, R::Error> {
        let bytes = encode(rec)?;
        fs::create_dir_all(&self.dir).map_err(|err| lift::<R>(err.into()))?;
        let id = self.free_id(rec.name()).map_err(lift::<R>)?;
        self.write_atomic(&id, &bytes).map_err(lift::<R>)?;
        Ok(id)
    }

    /// Replaces the existing record `id` with `rec`. The file must already
    /// exist; on any failure the previous contents are left intact.
    pub fn overwrite(&self, id: &str, rec: &R) -> Result<(), R::Error> {
        check_id(id).map_err(lift::<R>)?;
        let bytes = encode(rec)?;
        let exists = self
            .file(id, "json")
            .try_exists()
            .map_err(|err| lift::<R>(err.into()))?;
        if !exists {
            return Err(lift::<R>(MacroError::NotFound(id.to_owned())));
        }
        self.write_atomic(id, &bytes).map_err(lift::<R>)
    }

    fn file(&self, id: &str, extension: &str) -> PathBuf {
        self.dir.join(format!("{id}.{extension}"))
    }

    fn free_id(&self, name: &str) -> Result<String, MacroError> {
        let base = slug(name);
        for n in 1..=MAX_SUFFIX {
            let id = if n == 1 {
                base.clone()
            } else {
                format!("{base}-{n}")
            };
            if !self.file(&id, "json").try_exists()? && !self.file(&id, "json.bak").try_exists()? {
                return Ok(id);
            }
        }
        Err(MacroError::NoFreeId)
    }

    fn write_atomic(&self, id: &str, bytes: &[u8]) -> Result<(), MacroError> {
        let tmp = self.file(id, "json.tmp");
        let result =
            write_synced(&tmp, bytes).and_then(|()| replace_file(&tmp, &self.file(id, "json")));
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        Ok(result?)
    }

    /// A `.json.bak` without its `.json` means a crash inside the fallback swap;
    /// the backup is the last good copy, so move it back.
    fn restore_orphaned_backups(&self, names: &mut Vec<String>) {
        let orphans: Vec<String> = names
            .iter()
            .filter_map(|name| name.strip_suffix(".json.bak"))
            .filter(|id| check_id(id).is_ok() && !names.contains(&format!("{id}.json")))
            .map(str::to_owned)
            .collect();
        for id in orphans {
            if fs::rename(self.file(&id, "json.bak"), self.file(&id, "json")).is_ok() {
                names.push(format!("{id}.json"));
            }
        }
    }
}

/// Lift a store failure into the record's error type.
fn lift<R: Record>(err: MacroError) -> R::Error {
    err.into()
}

fn encode<R: Record>(rec: &R) -> Result<Vec<u8>, R::Error> {
    rec.validate()?;
    serde_json::to_vec(rec).map_err(|err| lift::<R>(err.into()))
}

/// Reads a whole file after checking its size against `max`.
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, MacroError> {
    let bytes = fs::metadata(path)?.len();
    if bytes > max {
        return Err(MacroError::FileTooLarge { bytes, max });
    }
    Ok(fs::read(path)?)
}

fn decode<R: Record>(data: &[u8]) -> Result<R, R::Error> {
    match serde_json::from_slice::<R>(data) {
        Ok(rec) => {
            rec.validate()?;
            Ok(rec)
        }
        Err(err) => {
            // A newer schema may not fit this layout; call that a version
            // problem rather than corruption.
            #[derive(Deserialize)]
            struct Header {
                schema_version: u32,
            }
            match serde_json::from_slice::<Header>(data) {
                Ok(header) if !R::SCHEMA_VERSIONS.contains(&header.schema_version) => {
                    Err(lift::<R>(MacroError::UnsupportedVersion {
                        found: header.schema_version,
                    }))
                }
                _ => Err(lift::<R>(err.into())),
            }
        }
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn replace_file(tmp: &Path, dest: &Path) -> io::Result<()> {
    let first = match fs::rename(tmp, dest) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };
    if dest.try_exists().unwrap_or(false) {
        replace_via_backup(tmp, dest)
    } else {
        Err(first)
    }
}

/// Fallback for filesystems whose rename will not replace an existing file.
fn replace_via_backup(tmp: &Path, dest: &Path) -> io::Result<()> {
    let backup = with_suffix(dest, ".bak");
    let _ = fs::remove_file(&backup);
    fs::rename(dest, &backup)?;
    if let Err(err) = fs::rename(tmp, dest) {
        let _ = fs::rename(&backup, dest);
        return Err(err);
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut os: OsString = path.as_os_str().to_owned();
    os.push(suffix);
    PathBuf::from(os)
}

fn check_id(id: &str) -> Result<(), MacroError> {
    let ok = !id.is_empty()
        && id.len() <= MAX_ID_CHARS
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(MacroError::InvalidId(id.to_owned()))
    }
}

/// Lowercase ASCII slug of a display name, `macro` if nothing survives.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= SLUG_CHARS {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("macro");
    }
    out
}

#[cfg(test)]
mod tests;
