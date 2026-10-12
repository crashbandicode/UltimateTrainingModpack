//! Persistent replay-capture storage and startup-only live-capture settings.
//! Native save success supplies the replay identity; this module never guesses it
//! from a filename appearing on disk or a controller button press.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc::{self, SyncSender}, Arc, OnceLock};

use training_mod_macros::{CaptureArchive, CaptureSettings, LiveSession, SharedCaptureArchive,
    DEFAULT_CAPTURE_INI, MAX_RETAINED_CAPTURES};

static LIVE_ENABLED: AtomicBool = AtomicBool::new(false);
static WRITER: OnceLock<SyncSender<CommittedCapture>> = OnceLock::new();

struct CommittedCapture {
    replay_id: String,
    replay_name: String,
    session: Arc<LiveSession>,
}

pub fn archive() -> CaptureArchive {
    CaptureArchive::new(format!(
        "{}/replay_captures",
        training_mod_consts::TRAINING_MODPACK_ROOT
    ))
}

pub fn enabled() -> bool {
    LIVE_ENABLED.load(Ordering::Acquire)
}

/// Start once, after the optional native adapter's guards pass. A single bounded
/// writer serializes archive publication; fighter callbacks never perform I/O.
pub fn start_writer() -> Result<(), String> {
    if WRITER.get().is_some() { return Ok(()); }
    let (sender, receiver) = mpsc::sync_channel::<CommittedCapture>(MAX_RETAINED_CAPTURES);
    std::thread::Builder::new().name("replay-capture-save".into()).stack_size(0x20000)
        .spawn(move || {
            let archive = SharedCaptureArchive::new(format!("{}/replay_captures",
                training_mod_consts::TRAINING_MODPACK_ROOT));
            while let Ok(saved) = receiver.recv() {
                match archive.save_with_outcome(&saved.replay_id, &saved.replay_name, "13.0.5", &saved.session) {
                    Ok(outcome) => println!("[ReplayCapture] {} id={} name={:?}",
                        if outcome.created { "persisted" } else { "already_present" },
                        outcome.id, saved.replay_name),
                    Err(error) => println!("[ReplayCapture] persistence_failed id={:?}: {}",
                        saved.replay_id, error),
                }
            }
        }).map_err(|error| error.to_string())?;
    // Startup owns initialization. Dropping a redundant sender also closes its
    // receiver, so even a repeated init cannot leave an idle extra worker behind.
    let _ = WRITER.set(sender);
    Ok(())
}

/// Only the verified committed-save boundary may submit a value-owned snapshot.
/// No native actor, string, or fighter pointer is sent to the background thread.
pub fn persist_committed(replay_id: String, replay_name: String, session: Arc<LiveSession>) {
    let Some(writer) = WRITER.get() else {
        println!("[ReplayCapture] persistence_writer_unavailable");
        return;
    };
    if let Err(error) = writer.try_send(CommittedCapture { replay_id, replay_name, session }) {
        println!("[ReplayCapture] persistence_queue_unavailable: {}", error);
    }
}

/// Called after the Training data directory and legacy migration are ready.
/// Configuration is read once; capture sampling performs no filesystem reads.
pub fn init() {
    let path = format!(
        "{}/replay_capture.ini",
        training_mod_consts::TRAINING_MODPACK_ROOT
    );
    let result = (|| -> Result<CaptureSettings, String> {
        match OpenOptions::new().write(true).create(true).create_new(true).truncate(false).open(&path) {
            Ok(mut file) => file
                .write_all(DEFAULT_CAPTURE_INI.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| error.to_string())?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
        if fs::metadata(&path).map_err(|error| error.to_string())?.len() > 16 * 1024 {
            return Err("replay_capture.ini exceeds 16 KiB".into());
        }
        CaptureSettings::parse(&fs::read_to_string(path).map_err(|error| error.to_string())?)
    })();
    match result {
        Ok(settings) => {
            LIVE_ENABLED.store(settings.enabled, Ordering::Release);
            println!("[ReplayCapture] live_enabled={}", enabled());
        }
        Err(error) => {
            LIVE_ENABLED.store(false, Ordering::Release);
            println!("[ReplayCapture] live capture disabled: {error}");
        }
    }
}
