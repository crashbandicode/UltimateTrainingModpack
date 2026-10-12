//! Bounded, portable storage for captured live-match replay observations.
//!
//! Capture writes happen after a match on a serialized worker. This module does
//! no per-frame I/O and never interprets native replay bytes.

use std::{
    fs::{self, File, FileType, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};

use crate::{
    InputFrame, ReplayFrame, ReplaySession, ReplayTrack, SkippedFile, StartState,
    MAX_FRAMES, MAX_GAME_VERSION_BYTES,
};

const MAGIC: &[u8; 8] = b"TMRCAP01";
const FIXED_HEADER_BYTES: usize = 12;
const CAPTURE_SCHEMA_VERSION: u32 = 1;
const TEMP_CREATE_ATTEMPTS: usize = 64;
static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Largest JSON metadata header, before any frame samples.
pub const MAX_CAPTURE_HEADER_BYTES: usize = 16 * 1024;
/// Largest number of concurrent fighter entries represented in a capture.
pub const MAX_CAPTURE_TRACKS: usize = 8;
/// Explicit binary record size: input (8), start state (28), and position (8).
pub const CAPTURE_RECORD_BYTES: usize = 44;
/// Maximum UTF-8 byte length of the native replay id stored in the header.
pub const MAX_CAPTURE_REPLAY_ID_BYTES: usize = 96;
/// Maximum UTF-8 byte length of the display name stored in the header.
pub const MAX_CAPTURE_REPLAY_NAME_BYTES: usize = 1024;
/// Hard file bound: fixed header + JSON header + every allowed track sample.
pub const MAX_CAPTURE_FILE_BYTES: u64 = (FIXED_HEADER_BYTES
    + MAX_CAPTURE_HEADER_BYTES
    + MAX_FRAMES * MAX_CAPTURE_TRACKS * CAPTURE_RECORD_BYTES) as u64;

/// Metadata shown by a replay picker. `id` is lowercase hex UTF-8 for the
/// original native replay id and is accepted by [`CaptureArchive::load`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureEntry {
    pub id: String,
    pub replay_name: String,
    pub game_version: String,
    pub tracks: usize,
    pub frames: usize,
    pub truncated: bool,
}

/// A fully decoded capture, including the original native replay identity.
#[derive(Clone, Debug)]
pub struct SavedCapture {
    pub replay_id: String,
    pub replay_name: String,
    pub game_version: String,
    pub session: ReplaySession,
}

/// A duplicate keeps the already-published capture; it did not persist new samples.
pub struct CaptureSaveOutcome {
    pub id: String,
    pub created: bool,
}

/// Valid capture metadata plus files that could not be listed safely.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureListing {
    /// Sorted by replay name (case-insensitive), then stable file id.
    pub entries: Vec<CaptureEntry>,
    pub skipped: Vec<SkippedFile>,
}

/// Portable replay-capture archive rooted at a caller-selected directory.
#[derive(Clone, Debug)]
pub struct CaptureArchive {
    dir: PathBuf,
}

impl CaptureArchive {
    /// Constructs an archive without performing I/O. The directory is created
    /// only when attempting to save a new capture.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Lists bounded metadata only; frame payloads are not read or validated.
    /// A missing directory is an empty archive and invalid files are reported
    /// in `skipped` without being changed or removed.
    pub fn list(&self) -> io::Result<CaptureListing> {
        let mut listing = CaptureListing::default();
        let read_dir = match fs::read_dir(&self.dir) {
            Ok(read_dir) => read_dir,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(listing),
            Err(err) => return Err(err),
        };

        for item in read_dir {
            let item = match item {
                Ok(item) => item,
                Err(err) => {
                    listing.skipped.push(SkippedFile {
                        file: String::new(),
                        reason: format!("unreadable directory entry: {err}"),
                    });
                    continue;
                }
            };
            let file = item.file_name().to_string_lossy().into_owned();
            let Some((id, shared)) = file.strip_suffix(".smd").map(|id| (id, true))
                .or_else(|| file.strip_suffix(".tmrc").map(|id| (id, false))) else {
                continue;
            };

            let result = (|| {
                decode_file_id(id).map_err(invalid_data)?;
                let path = if shared {
                    if !item.file_type()?.is_dir() {
                        return Err(invalid_data("shared capture path is not a directory"));
                    }
                    item.path().join(format!("{id}.tmrc"))
                } else {
                    if self.shared_cache_path(id)?.is_some() { return Ok(None); }
                    item.path()
                };
                if !shared && !item.file_type()?.is_file() {
                    return Err(invalid_data("capture path is not a regular file"));
                }
                let (_, header) = open_capture(&path, id)?;
                Ok(Some(CaptureEntry {
                    id: id.to_owned(),
                    replay_name: header.replay_name,
                    game_version: header.game_version,
                    tracks: header.tracks.len(),
                    frames: header
                        .tracks
                        .iter()
                        .map(|track| track.frames as usize)
                        .max()
                        .unwrap_or(0),
                    truncated: header.truncated,
                }))
            })();

            match result {
                Ok(Some(entry)) => listing.entries.push(entry),
                Ok(None) => {}
                Err(err) => listing.skipped.push(SkippedFile {
                    file,
                    reason: err.to_string(),
                }),
            }
        }

        listing.entries.sort_by(|a, b| {
            a.replay_name
                .to_lowercase()
                .cmp(&b.replay_name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(listing)
    }

    /// Loads a capture by its lowercase-hex file id and validates every sample.
    pub fn load(&self, id: &str) -> io::Result<SavedCapture> {
        let replay_id = decode_file_id(id).map_err(invalid_input)?;
        // The canonical bundle publishes this derived cache in the same rename.
        // Menu/frame code never parses the potentially large shared JSONL stream.
        let path = self.shared_cache_path(id)?.unwrap_or_else(|| self.capture_path(id));
        let (file, header) = open_capture(&path, id)?;
        let mut reader = BufReader::new(file);
        let mut tracks = Vec::with_capacity(header.tracks.len());

        for track_header in &header.tracks {
            let mut frames = Vec::with_capacity(track_header.frames as usize);
            for frame_index in 0..track_header.frames {
                let frame = read_frame(&mut reader)?;
                if let Some(problem) = frame_problem(&frame) {
                    return Err(invalid_data(format!(
                        "invalid sample in entry {} frame {}: {problem}",
                        track_header.entry, frame_index
                    )));
                }
                frames.push(frame);
            }
            tracks.push(ReplayTrack {
                entry: track_header.entry,
                frames,
                quantized_analog: track_header.quantized_analog,
            });
        }

        let mut trailing = [0u8; 1];
        if reader.read(&mut trailing)? != 0 {
            return Err(invalid_data("capture contains trailing sample bytes"));
        }

        Ok(SavedCapture {
            replay_id,
            replay_name: header.replay_name,
            game_version: header.game_version,
            session: ReplaySession {
                tracks,
                truncated: header.truncated,
            },
        })
    }

    /// Saves a new capture without replacing an existing replay id. Saves for
    /// this archive must be serialized by the caller. The returned value is
    /// the stable lowercase-hex filename id used by `load` and `list`.
    pub fn save(
        &self,
        replay_id: &str,
        replay_name: &str,
        game_version: &str,
        session: &ReplaySession,
    ) -> io::Result<String> {
        self.save_with_outcome(replay_id, replay_name, game_version, session).map(|saved| saved.id)
    }

    pub fn save_with_outcome(
        &self,
        replay_id: &str,
        replay_name: &str,
        game_version: &str,
        session: &ReplaySession,
    ) -> io::Result<CaptureSaveOutcome> {
        validate_replay_id(replay_id).map_err(invalid_input)?;
        validate_replay_name(replay_name).map_err(invalid_input)?;
        validate_game_version(game_version).map_err(invalid_input)?;
        let id = encode_file_id(replay_id);
        if let Some(path) = self.shared_cache_path(&id)? {
            ensure_existing_identity(&path, &id, replay_id, replay_name, game_version,
                fs::symlink_metadata(&path)?.file_type())?;
            return Ok(CaptureSaveOutcome { id, created: false });
        }
        let destination = self.capture_path(&id);

        if let Some(file_type) = file_type_if_exists(&destination)? {
            ensure_existing_identity(
                &destination,
                &id,
                replay_id,
                replay_name,
                game_version,
                file_type,
            )?;
            return Ok(CaptureSaveOutcome { id, created: false });
        }

        validate_session(session).map_err(invalid_input)?;
        let header = CaptureHeader {
            schema_version: CAPTURE_SCHEMA_VERSION,
            replay_id: replay_id.to_owned(),
            replay_name: replay_name.to_owned(),
            game_version: game_version.to_owned(),
            truncated: session.truncated,
            tracks: session
                .tracks
                .iter()
                .map(|track| TrackHeader {
                    entry: track.entry,
                    frames: track.frames.len() as u32,
                    quantized_analog: track.quantized_analog,
                })
                .collect(),
        };
        let header_bytes = serde_json::to_vec(&header)
            .map_err(|err| invalid_input(format!("could not encode capture header: {err}")))?;
        if header_bytes.len() > MAX_CAPTURE_HEADER_BYTES {
            return Err(invalid_input(format!(
                "capture header is {} bytes (maximum {MAX_CAPTURE_HEADER_BYTES})",
                header_bytes.len()
            )));
        }
        let expected_len = expected_file_len(header_bytes.len(), &header.tracks)?;
        if expected_len > MAX_CAPTURE_FILE_BYTES {
            return Err(invalid_input(format!(
                "capture would be {expected_len} bytes (maximum {MAX_CAPTURE_FILE_BYTES})"
            )));
        }

        fs::create_dir_all(&self.dir)?;
        let temporary = write_capture_temp(&self.dir, &id, &header_bytes, session)?;

        match file_type_if_exists(&destination) {
            Ok(Some(file_type)) => {
                let _ = fs::remove_file(&temporary);
                ensure_existing_identity(
                    &destination,
                    &id,
                    replay_id,
                    replay_name,
                    game_version,
                    file_type,
                )?;
                Ok(CaptureSaveOutcome { id, created: false })
            }
            Ok(None) => match fs::rename(&temporary, &destination) {
                Ok(()) => Ok(CaptureSaveOutcome { id, created: true }),
                Err(err) => {
                    let _ = fs::remove_file(&temporary);
                    Err(err)
                }
            },
            Err(err) => {
                let _ = fs::remove_file(&temporary);
                Err(err)
            }
        }
    }

    fn capture_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.tmrc"))
    }

    fn shared_cache_path(&self, id: &str) -> io::Result<Option<PathBuf>> {
        let dir = self.dir.join(format!("{id}.smd"));
        match file_type_if_exists(&dir)? {
            Some(kind) if kind.is_dir() => Ok(Some(dir.join(format!("{id}.tmrc")))),
            Some(_) => Err(invalid_data("shared capture path is not a directory")),
            None => Ok(None),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CaptureHeader {
    schema_version: u32,
    replay_id: String,
    replay_name: String,
    game_version: String,
    truncated: bool,
    tracks: Vec<TrackHeader>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct TrackHeader {
    entry: i32,
    frames: u32,
    quantized_analog: bool,
}

#[derive(Deserialize)]
struct HeaderVersion {
    schema_version: u32,
}

fn open_capture(path: &Path, file_id: &str) -> io::Result<(File, CaptureHeader)> {
    let path_type = fs::symlink_metadata(path)?.file_type();
    if !path_type.is_file() {
        return Err(invalid_data("capture path is not a regular file"));
    }
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let header = read_capture_header(&mut file, file_len, file_id)?;
    Ok((file, header))
}

fn read_capture_header(
    file: &mut File,
    file_len: u64,
    file_id: &str,
) -> io::Result<CaptureHeader> {
    if file_len > MAX_CAPTURE_FILE_BYTES {
        return Err(invalid_data(format!(
            "capture file is {file_len} bytes (maximum {MAX_CAPTURE_FILE_BYTES})"
        )));
    }
    if file_len < FIXED_HEADER_BYTES as u64 {
        return Err(invalid_data("truncated capture file header"));
    }

    let mut fixed = [0u8; FIXED_HEADER_BYTES];
    read_exact_capture(file, &mut fixed, "fixed header")?;
    if &fixed[..MAGIC.len()] != MAGIC {
        return Err(invalid_data("invalid capture magic"));
    }
    let header_len = u32::from_le_bytes([fixed[8], fixed[9], fixed[10], fixed[11]]) as usize;
    if header_len == 0 || header_len > MAX_CAPTURE_HEADER_BYTES {
        return Err(invalid_data(format!(
            "capture header length {header_len} is outside 1..={MAX_CAPTURE_HEADER_BYTES}"
        )));
    }
    if file_len < (FIXED_HEADER_BYTES + header_len) as u64 {
        return Err(invalid_data("truncated capture JSON header"));
    }

    let mut header_bytes = vec![0u8; header_len];
    read_exact_capture(file, &mut header_bytes, "JSON header")?;
    let version: HeaderVersion = serde_json::from_slice(&header_bytes)
        .map_err(|err| invalid_data(format!("malformed capture JSON header: {err}")))?;
    if version.schema_version != CAPTURE_SCHEMA_VERSION {
        return Err(invalid_data(format!(
            "unsupported capture schema version {} (supported: {CAPTURE_SCHEMA_VERSION})",
            version.schema_version
        )));
    }
    let header: CaptureHeader = serde_json::from_slice(&header_bytes)
        .map_err(|err| invalid_data(format!("malformed capture JSON header: {err}")))?;
    validate_header(&header, file_id)?;

    let expected_len = expected_file_len(header_len, &header.tracks)?;
    if expected_len != file_len {
        return Err(if expected_len > file_len {
            invalid_data(format!(
                "truncated capture samples: expected {expected_len} bytes, found {file_len}"
            ))
        } else {
            invalid_data(format!(
                "capture has trailing bytes: expected {expected_len}, found {file_len}"
            ))
        });
    }
    Ok(header)
}

fn validate_header(header: &CaptureHeader, file_id: &str) -> io::Result<()> {
    if header.schema_version != CAPTURE_SCHEMA_VERSION {
        return Err(invalid_data(format!(
            "unsupported capture schema version {} (supported: {CAPTURE_SCHEMA_VERSION})",
            header.schema_version
        )));
    }
    validate_replay_id(&header.replay_id).map_err(invalid_data)?;
    validate_replay_name(&header.replay_name).map_err(invalid_data)?;
    validate_game_version(&header.game_version).map_err(invalid_data)?;
    if encode_file_id(&header.replay_id) != file_id {
        return Err(invalid_data(
            "capture replay id does not encode to its filename",
        ));
    }
    if header.tracks.is_empty() || header.tracks.len() > MAX_CAPTURE_TRACKS {
        return Err(invalid_data(format!(
            "capture must have 1..={MAX_CAPTURE_TRACKS} tracks"
        )));
    }

    let mut entries = [false; MAX_CAPTURE_TRACKS];
    for track in &header.tracks {
        if !(0..MAX_CAPTURE_TRACKS as i32).contains(&track.entry) {
            return Err(invalid_data(format!(
                "capture entry {} is outside 0..{}",
                track.entry,
                MAX_CAPTURE_TRACKS - 1
            )));
        }
        let entry = track.entry as usize;
        if std::mem::replace(&mut entries[entry], true) {
            return Err(invalid_data(format!(
                "duplicate capture entry {}",
                track.entry
            )));
        }
        if track.frames == 0 || u64::from(track.frames) > MAX_FRAMES as u64 {
            return Err(invalid_data(format!(
                "entry {} has {} frames (allowed: 1..={MAX_FRAMES})",
                track.entry, track.frames
            )));
        }
    }
    Ok(())
}

fn expected_file_len(header_len: usize, tracks: &[TrackHeader]) -> io::Result<u64> {
    let sample_bytes = tracks
        .iter()
        .try_fold(0u64, |total, track| {
            total.checked_add(u64::from(track.frames).checked_mul(CAPTURE_RECORD_BYTES as u64)?)
        })
        .ok_or_else(|| invalid_data("capture size calculation overflowed"))?;
    (FIXED_HEADER_BYTES as u64)
        .checked_add(header_len as u64)
        .and_then(|total| total.checked_add(sample_bytes))
        .ok_or_else(|| invalid_data("capture size calculation overflowed"))
}

pub(crate) fn validate_replay_id(replay_id: &str) -> Result<(), &'static str> {
    if replay_id.is_empty() {
        return Err("replay id is empty");
    }
    if replay_id.len() > MAX_CAPTURE_REPLAY_ID_BYTES {
        return Err("replay id exceeds 96 UTF-8 bytes");
    }
    if replay_id.chars().any(char::is_control) {
        return Err("replay id contains control characters");
    }
    Ok(())
}

pub(crate) fn validate_replay_name(replay_name: &str) -> Result<(), &'static str> {
    if replay_name.is_empty() {
        return Err("replay name is empty");
    }
    if replay_name.len() > MAX_CAPTURE_REPLAY_NAME_BYTES {
        return Err("replay name exceeds 1024 UTF-8 bytes");
    }
    if replay_name.chars().any(char::is_control) {
        return Err("replay name contains control characters");
    }
    Ok(())
}

pub(crate) fn validate_game_version(game_version: &str) -> Result<(), &'static str> {
    if game_version.len() > MAX_GAME_VERSION_BYTES {
        return Err("game version exceeds 32 UTF-8 bytes");
    }
    if game_version.chars().any(char::is_control) {
        return Err("game version contains control characters");
    }
    Ok(())
}

fn validate_session(session: &ReplaySession) -> Result<(), String> {
    if session.tracks.is_empty() || session.tracks.len() > MAX_CAPTURE_TRACKS {
        return Err(format!(
            "capture must have 1..={MAX_CAPTURE_TRACKS} tracks"
        ));
    }
    let mut entries = [false; MAX_CAPTURE_TRACKS];
    for track in &session.tracks {
        if !(0..MAX_CAPTURE_TRACKS as i32).contains(&track.entry) {
            return Err(format!(
                "capture entry {} is outside 0..{}",
                track.entry,
                MAX_CAPTURE_TRACKS - 1
            ));
        }
        let entry = track.entry as usize;
        if std::mem::replace(&mut entries[entry], true) {
            return Err(format!("duplicate capture entry {}", track.entry));
        }
        if track.frames.is_empty() || track.frames.len() > MAX_FRAMES {
            return Err(format!(
                "entry {} has {} frames (allowed: 1..={MAX_FRAMES})",
                track.entry,
                track.frames.len()
            ));
        }
        for (frame_index, frame) in track.frames.iter().enumerate() {
            if let Some(problem) = frame_problem(frame) {
                return Err(format!(
                    "invalid sample in entry {} frame {frame_index}: {problem}",
                    track.entry
                ));
            }
        }
    }
    Ok(())
}

fn frame_problem(frame: &ReplayFrame) -> Option<&'static str> {
    if frame.start.fighter_kind < 0 {
        return Some("fighter kind is negative");
    }
    if !frame.start.facing.is_finite() {
        return Some("facing is not finite");
    }
    if !frame.start.speed_x.is_finite() {
        return Some("horizontal speed is not finite");
    }
    if !frame.start.speed_y.is_finite() {
        return Some("vertical speed is not finite");
    }
    if !frame.x.is_finite() || !frame.y.is_finite() {
        return Some("position is not finite");
    }
    None
}

fn ensure_existing_identity(
    path: &Path,
    file_id: &str,
    replay_id: &str,
    replay_name: &str,
    game_version: &str,
    file_type: FileType,
) -> io::Result<()> {
    if !file_type.is_file() {
        return Err(invalid_data("capture path already exists and is not a regular file"));
    }
    let (_, header) = open_capture(path, file_id)?;
    if header.replay_id != replay_id
        || header.replay_name != replay_name
        || header.game_version != game_version
    {
        return Err(invalid_data(
            "capture id already exists with incompatible replay metadata",
        ));
    }
    Ok(())
}

fn file_type_if_exists(path: &Path) -> io::Result<Option<FileType>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn write_capture_temp(
    dir: &Path,
    file_id: &str,
    header_bytes: &[u8],
    session: &ReplaySession,
) -> io::Result<PathBuf> {
    for _ in 0..TEMP_CREATE_ATTEMPTS {
        let counter = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        // Switch std has no process IDs. Exclusive create plus a counter also
        // avoids stale temporary files retained across process restarts.
        let path = dir.join(format!(".{file_id}.{counter}.tmp"));
        // Switch std requires create as well as create_new for a missing path.
        let file = match OpenOptions::new().write(true).create(true).create_new(true).truncate(false).open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        };

        let result = (|| {
            let mut writer = BufWriter::with_capacity(64 * 1024, file);
            writer.write_all(MAGIC)?;
            writer.write_all(&(header_bytes.len() as u32).to_le_bytes())?;
            writer.write_all(header_bytes)?;
            for track in &session.tracks {
                for frame in &track.frames {
                    write_frame(&mut writer, frame)?;
                }
            }
            writer.flush()?;
            writer.get_ref().sync_all()
        })();
        if let Err(err) = result {
            let _ = fs::remove_file(&path);
            return Err(err);
        }
        return Ok(path);
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate an exclusive capture temporary file",
    ))
}

fn write_frame(writer: &mut impl Write, frame: &ReplayFrame) -> io::Result<()> {
    let mut record = [0u8; CAPTURE_RECORD_BYTES];
    record[0..4].copy_from_slice(&frame.input.buttons.to_le_bytes());
    record[4] = frame.input.lstick_x as u8;
    record[5] = frame.input.lstick_y as u8;
    record[6] = frame.input.rstick_x as u8;
    record[7] = frame.input.rstick_y as u8;
    record[8..12].copy_from_slice(&frame.start.fighter_kind.to_le_bytes());
    record[12..16].copy_from_slice(&frame.start.costume.to_le_bytes());
    record[16..20].copy_from_slice(&frame.start.facing.to_le_bytes());
    record[20..24].copy_from_slice(&frame.start.situation.to_le_bytes());
    record[24..28].copy_from_slice(&frame.start.status.to_le_bytes());
    record[28..32].copy_from_slice(&frame.start.speed_x.to_le_bytes());
    record[32..36].copy_from_slice(&frame.start.speed_y.to_le_bytes());
    record[36..40].copy_from_slice(&frame.x.to_le_bytes());
    record[40..44].copy_from_slice(&frame.y.to_le_bytes());
    writer.write_all(&record)
}

fn read_frame(reader: &mut impl Read) -> io::Result<ReplayFrame> {
    let mut record = [0u8; CAPTURE_RECORD_BYTES];
    read_exact_capture(reader, &mut record, "sample data")?;
    let frame = ReplayFrame {
        input: InputFrame {
            buttons: read_u32(&record, 0),
            lstick_x: record[4] as i8,
            lstick_y: record[5] as i8,
            rstick_x: record[6] as i8,
            rstick_y: record[7] as i8,
        },
        start: StartState {
            fighter_kind: read_i32(&record, 8),
            costume: read_u32(&record, 12),
            facing: read_f32(&record, 16),
            situation: read_i32(&record, 20),
            status: read_i32(&record, 24),
            speed_x: read_f32(&record, 28),
            speed_y: read_f32(&record, 32),
        },
        x: read_f32(&record, 36),
        y: read_f32(&record, 40),
    };
    Ok(frame)
}

fn read_u32(record: &[u8; CAPTURE_RECORD_BYTES], offset: usize) -> u32 {
    u32::from_le_bytes([
        record[offset],
        record[offset + 1],
        record[offset + 2],
        record[offset + 3],
    ])
}

fn read_i32(record: &[u8; CAPTURE_RECORD_BYTES], offset: usize) -> i32 {
    i32::from_le_bytes([
        record[offset],
        record[offset + 1],
        record[offset + 2],
        record[offset + 3],
    ])
}

fn read_f32(record: &[u8; CAPTURE_RECORD_BYTES], offset: usize) -> f32 {
    f32::from_bits(read_u32(record, offset))
}

fn read_exact_capture(reader: &mut impl Read, buffer: &mut [u8], part: &str) -> io::Result<()> {
    reader.read_exact(buffer).map_err(|err| {
        if err.kind() == io::ErrorKind::UnexpectedEof {
            invalid_data(format!("truncated capture {part}"))
        } else {
            err
        }
    })
}

pub(crate) fn encode_file_id(replay_id: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(replay_id.len() * 2);
    for byte in replay_id.as_bytes() {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

pub(crate) fn decode_file_id(file_id: &str) -> Result<String, &'static str> {
    if file_id.is_empty()
        || file_id.len() > MAX_CAPTURE_REPLAY_ID_BYTES * 2
        || !file_id.len().is_multiple_of(2)
    {
        return Err("capture file id must be even-length lowercase hex for 1..=96 bytes");
    }
    let mut decoded = Vec::with_capacity(file_id.len() / 2);
    for pair in file_id.as_bytes().chunks_exact(2) {
        let high = hex_nibble(pair[0]).ok_or("capture file id is not lowercase hex")?;
        let low = hex_nibble(pair[1]).ok_or("capture file id is not lowercase hex")?;
        decoded.push((high << 4) | low);
    }
    let replay_id = String::from_utf8(decoded).map_err(|_| "capture file id is not UTF-8")?;
    validate_replay_id(&replay_id)?;
    if encode_file_id(&replay_id) != file_id {
        return Err("capture file id is not canonical lowercase hex");
    }
    Ok(replay_id)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom};

    static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "training-mod-capture-{}-{}",
                std::process::id(),
                NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sample_session() -> ReplaySession {
        let first = ReplayFrame {
            input: InputFrame {
                buttons: 0xa1b2_c3d4,
                lstick_x: -127,
                lstick_y: 64,
                rstick_x: 12,
                rstick_y: -9,
            },
            start: StartState {
                fighter_kind: 18,
                costume: 3,
                facing: -1.0,
                situation: 2,
                status: 0x1234,
                speed_x: 1.25,
                speed_y: -2.5,
            },
            x: -17.25,
            y: 21.5,
        };
        let mut second = first;
        second.input.buttons = 0x5566_7788;
        second.start.status = 99;
        second.x = 8.5;
        second.y = -0.25;
        ReplaySession {
            tracks: vec![
                ReplayTrack {
                    entry: 3,
                    frames: vec![first, second],
                    quantized_analog: true,
                },
                ReplayTrack {
                    entry: 0,
                    frames: vec![first],
                    quantized_analog: false,
                },
            ],
            truncated: true,
        }
    }

    fn assert_frame_eq(left: &ReplayFrame, right: &ReplayFrame) {
        assert_eq!(left.input, right.input);
        assert_eq!(left.start, right.start);
        assert_eq!(left.x, right.x);
        assert_eq!(left.y, right.y);
    }

    fn capture_file(header: &[u8]) -> Vec<u8> {
        let mut file = Vec::with_capacity(FIXED_HEADER_BYTES + header.len());
        file.extend_from_slice(MAGIC);
        file.extend_from_slice(&(header.len() as u32).to_le_bytes());
        file.extend_from_slice(header);
        file
    }

    #[test]
    fn saves_and_reopens_explicit_samples_and_truncation_metadata() {
        let temp = TestDir::new();
        let archive = CaptureArchive::new(&temp.0);
        let original = sample_session();
        let id = archive
            .save("native-replay-雪", "Training session 雪", "13.0.5", &original)
            .unwrap();
        let loaded = archive.load(&id).unwrap();

        assert_eq!(loaded.replay_id, "native-replay-雪");
        assert_eq!(loaded.replay_name, "Training session 雪");
        assert_eq!(loaded.game_version, "13.0.5");
        assert!(loaded.session.truncated);
        assert_eq!(loaded.session.tracks.len(), 2);
        for (expected, actual) in original.tracks.iter().zip(&loaded.session.tracks) {
            assert_eq!(actual.entry, expected.entry);
            assert_eq!(actual.quantized_analog, expected.quantized_analog);
            assert_eq!(actual.frames.len(), expected.frames.len());
            for (expected_frame, actual_frame) in expected.frames.iter().zip(&actual.frames) {
                assert_frame_eq(expected_frame, actual_frame);
            }
        }
    }

    #[test]
    fn ids_are_separate_from_duplicate_unicode_display_names() {
        let temp = TestDir::new();
        let archive = CaptureArchive::new(&temp.0);
        let first = archive
            .save("原生 id 一", "Shared 名", "v1", &sample_session())
            .unwrap();
        let second = archive
            .save("native/id-two", "Shared 名", "v1", &sample_session())
            .unwrap();
        let third = archive
            .save("a-native-id", "shared 名", "v1", &sample_session())
            .unwrap();

        assert_ne!(first, second);
        assert_eq!(first, encode_file_id("原生 id 一"));
        assert_ne!(first, "Shared 名");
        assert_eq!(archive.load(&first).unwrap().replay_id, "原生 id 一");
        assert_eq!(archive.load(&second).unwrap().replay_id, "native/id-two");
        assert_eq!(archive.load(&third).unwrap().replay_id, "a-native-id");
        let listing = archive.list().unwrap();
        assert_eq!(listing.entries.len(), 3);
        assert!(listing
            .entries
            .iter()
            .all(|entry| entry.replay_name.to_lowercase() == "shared 名"));
        let mut expected_ids = vec![first, second, third];
        expected_ids.sort();
        assert_eq!(
            listing.entries.iter().map(|entry| entry.id.clone()).collect::<Vec<_>>(),
            expected_ids
        );
    }

    #[test]
    fn duplicate_save_preserves_published_bytes_and_rejects_other_identity() {
        let temp = TestDir::new();
        let archive = CaptureArchive::new(&temp.0);
        let first_session = sample_session();
        let id = archive
            .save("native-id", "Same replay", "v1", &first_session)
            .unwrap();
        let path = temp.0.join(format!("{id}.tmrc"));
        let before = fs::read(&path).unwrap();

        let mut later_session = sample_session();
        later_session.tracks[0].frames[0].input.buttons = 0xdead_beef;
        assert_eq!(
            archive
                .save("native-id", "Same replay", "v1", &later_session)
                .unwrap(),
            id
        );
        assert!(archive
            .save("native-id", "Renamed replay", "v1", &later_session)
            .is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(
            archive.load(&id).unwrap().session.tracks[0].frames[0]
                .input
                .buttons,
            first_session.tracks[0].frames[0].input.buttons
        );
    }

    #[test]
    fn rejects_invalid_game_version_on_save_before_creating_the_archive() {
        let temp = TestDir::new();
        let archive_dir = temp.0.join("archive");
        let archive = CaptureArchive::new(&archive_dir);
        let too_long = "v".repeat(MAX_GAME_VERSION_BYTES + 1);
        for game_version in [too_long.as_str(), "bad\u{7}version"] {
            let err = archive
                .save("native-id", "A replay", game_version, &sample_session())
                .unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        }
        assert!(!archive_dir.exists());
    }

    #[test]
    fn rejects_invalid_game_versions_in_headers_without_removing_files() {
        let temp = TestDir::new();
        let archive = CaptureArchive::new(&temp.0);
        let too_long = "v".repeat(MAX_GAME_VERSION_BYTES + 1);
        let invalid_versions = [
            ("long-version", too_long),
            ("control-version", "bad\u{7}version".to_owned()),
        ];

        for (replay_id, game_version) in &invalid_versions {
            let header = CaptureHeader {
                schema_version: CAPTURE_SCHEMA_VERSION,
                replay_id: (*replay_id).to_owned(),
                replay_name: "Bad game version".to_owned(),
                game_version: game_version.as_str().to_owned(),
                truncated: false,
                tracks: Vec::new(),
            };
            let bytes = capture_file(&serde_json::to_vec(&header).unwrap());
            fs::write(
                temp.0
                    .join(format!("{}.tmrc", encode_file_id(replay_id))),
                bytes,
            )
            .unwrap();
        }

        let listing = archive.list().unwrap();
        assert!(listing.entries.is_empty());
        assert_eq!(listing.skipped.len(), 2);
        assert!(listing
            .skipped
            .iter()
            .all(|file| file.reason.contains("game version")));
        for (replay_id, _) in &invalid_versions {
            let id = encode_file_id(replay_id);
            assert!(archive.load(&id).is_err());
            assert!(temp.0.join(format!("{id}.tmrc")).exists());
        }
    }

    #[test]
    fn listing_uses_header_only_while_load_validates_samples() {
        let temp = TestDir::new();
        let archive = CaptureArchive::new(&temp.0);
        let id = archive
            .save("metadata-only", "Header entry", "v1", &sample_session())
            .unwrap();
        let path = temp.0.join(format!("{id}.tmrc"));
        let bytes = fs::read(&path).unwrap();
        let header_len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as u64;
        let first_x = FIXED_HEADER_BYTES as u64 + header_len + 36;
        let mut file = OpenOptions::new().write(true).open(&path).unwrap();
        file.seek(SeekFrom::Start(first_x)).unwrap();
        file.write_all(&f32::NAN.to_le_bytes()).unwrap();
        file.sync_all().unwrap();

        let listing = archive.list().unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].frames, 2);
        assert!(listing.skipped.is_empty());
        assert_eq!(archive.load(&id).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(path.exists());
    }

    #[test]
    fn malformed_truncated_oversized_and_unsupported_files_are_reported_and_kept() {
        let temp = TestDir::new();
        let archive = CaptureArchive::new(&temp.0);
        fs::create_dir_all(&temp.0).unwrap();

        let truncated_id = encode_file_id("truncated");
        fs::write(
            temp.0.join(format!("{truncated_id}.tmrc")),
            &MAGIC[..],
        )
        .unwrap();
        let malformed_id = encode_file_id("malformed");
        fs::write(
            temp.0.join(format!("{malformed_id}.tmrc")),
            capture_file(b"{not json"),
        )
        .unwrap();
        let unsupported_id = encode_file_id("unsupported");
        fs::write(
            temp.0.join(format!("{unsupported_id}.tmrc")),
            capture_file(br#"{"schema_version":2}"#),
        )
        .unwrap();
        let oversized_id = encode_file_id("oversized");
        let oversized_path = temp.0.join(format!("{oversized_id}.tmrc"));
        File::create(&oversized_path)
            .unwrap()
            .set_len(MAX_CAPTURE_FILE_BYTES + 1)
            .unwrap();

        let listing = archive.list().unwrap();
        assert!(listing.entries.is_empty());
        assert_eq!(listing.skipped.len(), 4);
        let reasons = listing
            .skipped
            .iter()
            .map(|file| file.reason.as_str())
            .collect::<Vec<_>>();
        assert!(reasons.iter().any(|reason| reason.contains("truncated")));
        assert!(reasons.iter().any(|reason| reason.contains("malformed")));
        assert!(reasons.iter().any(|reason| reason.contains("unsupported")));
        assert!(reasons.iter().any(|reason| reason.contains("maximum")));
        for id in [truncated_id, malformed_id, unsupported_id, oversized_id] {
            assert!(temp.0.join(format!("{id}.tmrc")).exists());
            assert!(archive.load(&id).is_err());
        }
    }
}
