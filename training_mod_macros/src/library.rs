//! Directory-backed macro library: one compact JSON file per macro, named
//! `<id>.json`, in a caller-chosen directory. There is no entry-count limit.
//!
//! * Ids are stable and path-safe (`[a-z0-9_-]`, at most [`MAX_ID_CHARS`]). The
//!   id is the file stem; it never changes when the macro is overwritten.
//! * Updates write `<id>.json.tmp`, sync it, then rename it over the target, so
//!   a reader or an interrupted write never sees a partial file. Where rename
//!   cannot replace an existing file (FAT on Horizon is the concern), the old
//!   file is moved to `<id>.json.bak` first and put back if the swap fails.
//! * `list` never deletes or rewrites anything except restoring an orphaned
//!   `.bak` (left only by a crash inside that swap).

use std::{
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::Deserialize;

use crate::recording::{MacroError, MacroRecording, SCHEMA_VERSION, MII_SCHEMA_VERSION};

/// Longest id accepted, in characters.
pub const MAX_ID_CHARS: usize = 48;
/// Largest file read: 8 MiB. A worst-case `MAX_FRAMES` macro is about 3 MB as
/// written here and about 5 MB pretty-printed, so hand-edited files still load
/// while anything far larger is rejected before it is read.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

const SLUG_CHARS: usize = 40;
const MAX_SUFFIX: u32 = 9_999;

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

/// Result of scanning the library: good entries plus the files that were skipped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    /// Sorted by name (case-insensitive), then id.
    pub entries: Vec<MacroEntry>,
    pub skipped: Vec<SkippedFile>,
}

/// A macro library rooted at a caller-supplied directory.
#[derive(Clone, Debug)]
pub struct Library {
    dir: PathBuf,
}

impl Library {
    /// Does no I/O; the directory is created by the first save.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Lists every valid macro. A missing directory is an empty library.
    /// Corrupt or invalid files are reported in `skipped` and left untouched.
    pub fn list(&self) -> Result<Listing, MacroError> {
        let mut listing = Listing::default();
        let read_dir = match fs::read_dir(&self.dir) {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(listing),
            Err(err) => return Err(err.into()),
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
        for name in names {
            let Some(id) = name.strip_suffix(".json") else {
                continue;
            };
            let result = check_id(id).and_then(|()| read_file(&self.file(id, "json")));
            match result {
                Ok(rec) => listing.entries.push(MacroEntry {
                    id: id.to_owned(),
                    name: rec.name,
                    fighter_kind: rec.start.fighter_kind,
                    costume: rec.start.costume,
                    frame_count: rec.frames.len(),
                }),
                Err(err) => listing.skipped.push(SkippedFile {
                    file: name.clone(),
                    reason: err.to_string(),
                }),
            }
        }
        listing
            .entries
            .sort_by(|a, b| (a.name.to_lowercase(), &a.id).cmp(&(b.name.to_lowercase(), &b.id)));
        Ok(listing)
    }

    /// Loads and fully validates the macro with this id.
    pub fn load(&self, id: &str) -> Result<MacroRecording, MacroError> {
        check_id(id)?;
        read_file(&self.file(id, "json")).map_err(|err| match err {
            MacroError::Io(io) if io.kind() == io::ErrorKind::NotFound => {
                MacroError::NotFound(id.to_owned())
            }
            other => other,
        })
    }

    /// Saves as a brand-new file and returns its id. The id comes from the
    /// name (`my-drill`, then `my-drill-2`, ...) and never matches an existing
    /// file, good or corrupt, so nothing already on disk can be replaced.
    pub fn save_new(&self, rec: &MacroRecording) -> Result<String, MacroError> {
        let bytes = encode(rec)?;
        fs::create_dir_all(&self.dir)?;
        let id = self.free_id(&rec.name)?;
        self.write_atomic(&id, &bytes)?;
        Ok(id)
    }

    /// Replaces the existing macro `id` with `rec`. The file must already
    /// exist; on any failure the previous contents are left intact.
    pub fn overwrite(&self, id: &str, rec: &MacroRecording) -> Result<(), MacroError> {
        check_id(id)?;
        let bytes = encode(rec)?;
        if !self.file(id, "json").try_exists()? {
            return Err(MacroError::NotFound(id.to_owned()));
        }
        self.write_atomic(id, &bytes)
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

fn encode(rec: &MacroRecording) -> Result<Vec<u8>, MacroError> {
    rec.validate()?;
    Ok(serde_json::to_vec(rec)?)
}

fn read_file(path: &Path) -> Result<MacroRecording, MacroError> {
    let bytes = fs::metadata(path)?.len();
    if bytes > MAX_FILE_BYTES {
        return Err(MacroError::FileTooLarge {
            bytes,
            max: MAX_FILE_BYTES,
        });
    }
    decode(&fs::read(path)?)
}

fn decode(data: &[u8]) -> Result<MacroRecording, MacroError> {
    match serde_json::from_slice::<MacroRecording>(data) {
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
                Ok(header) if !matches!(header.schema_version, SCHEMA_VERSION | MII_SCHEMA_VERSION) => {
                    Err(MacroError::UnsupportedVersion {
                        found: header.schema_version,
                    })
                }
                _ => Err(err.into()),
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
