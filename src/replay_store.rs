//! Persistent replay-capture storage and the live-capture preference.
//!
//! The INI is the single startup authority: it seeds the runtime preference once
//! and is never read while sampling. With the `deluxe-sdk` feature and a Deluxe
//! host present, the Plugins page changes the in-memory preference at runtime
//! and the host rewrites the same INI after accepted edits when its layout is
//! persistence-compatible. Native save success supplies the replay identity;
//! this module never guesses it from a filename appearing on disk or a
//! controller button press.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc::{self, SyncSender}, Arc, OnceLock};

use training_mod_macros::{recover_previous_ini, CaptureArchive, CaptureSettings, CaptureToggle,
    IniRecovery, LiveSession, SharedCaptureArchive, DEFAULT_CAPTURE_INI, MAX_RETAINED_CAPTURES};

/// The file Training reads at startup and the Deluxe host rewrites after menu
/// edits. Spelled out because the plugin descriptor needs a `'static` literal.
pub const REPLAY_CAPTURE_INI: &str = "sd:/ultimate/TrainingModpack/replay_capture.ini";
const _: () = assert!(
    path_under_root(REPLAY_CAPTURE_INI, training_mod_consts::TRAINING_MODPACK_ROOT, "/replay_capture.ini"),
    "REPLAY_CAPTURE_INI must be TRAINING_MODPACK_ROOT + /replay_capture.ini"
);

const fn path_under_root(path: &str, root: &str, suffix: &str) -> bool {
    let (path, root, suffix) = (path.as_bytes(), root.as_bytes(), suffix.as_bytes());
    if path.len() != root.len() + suffix.len() { return false; }
    let mut i = 0;
    while i < path.len() {
        let expected = if i < root.len() { root[i] } else { suffix[i - root.len()] };
        if path[i] != expected { return false; }
        i += 1;
    }
    true
}

/// INI value read at startup; `toggle()` holds the runtime preference.
static SEED_ENABLED: AtomicBool = AtomicBool::new(false);
static PERSISTENCE_COMPATIBLE: AtomicBool = AtomicBool::new(false);
static TOGGLE: CaptureToggle = CaptureToggle::new();
static WRITER: OnceLock<SyncSender<WriteJob>> = OnceLock::new();

struct CommittedCapture {
    replay_id: String,
    replay_name: String,
    session: Arc<LiveSession>,
    miis: Option<training_mod_consts::mii::replay::ReplayMiis>,
}

enum WriteJob {
    Capture(Box<CommittedCapture>),
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    Catalog(training_mod_consts::mii::inventory::Snapshot,
        mpsc::Sender<training_mod_consts::mii::inventory::Snapshot>),
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    Import { name: Option<String>, configuration: training_mod_consts::mii::lifetime::Configuration,
        native: Arc<training_mod_consts::mii::inventory::Snapshot>,
        reply: mpsc::Sender<Result<Arc<training_mod_consts::mii::draft::Library>, String>> },
}

pub fn archive() -> CaptureArchive {
    CaptureArchive::new(format!(
        "{}/replay_captures",
        training_mod_consts::TRAINING_MODPACK_ROOT
    ))
}

/// Packed runtime preference and record latch, shared by the Deluxe row (draw
/// thread) and the native adapter (game thread).
pub fn toggle() -> &'static CaptureToggle {
    &TOGGLE
}

/// What the INI said at startup. Runtime edits live in [`toggle`].
pub fn seed_enabled() -> bool {
    SEED_ENABLED.load(Ordering::Acquire)
}

/// Whether the Deluxe host may rewrite the INI (see
/// `CaptureSettings::persistence_compatible`); otherwise the row is session-only.
/// Only the `deluxe-sdk` registration reads it; the verdict is logged regardless.
#[cfg_attr(not(feature = "deluxe-sdk"), allow(dead_code))]
pub fn persistence_compatible() -> bool {
    PERSISTENCE_COMPATIBLE.load(Ordering::Acquire)
}

/// Start once for the mod library (combined alpha) or after live-capture guards.
/// A single bounded writer serializes publication; fighter callbacks never perform I/O.
pub fn start_writer() -> Result<(), String> {
    if WRITER.get().is_some() { return Ok(()); }
    let (sender, receiver) = mpsc::sync_channel::<WriteJob>(MAX_RETAINED_CAPTURES);
    std::thread::Builder::new().name("replay-capture-save".into()).stack_size(0x20000)
        .spawn(move || {
            let archive = SharedCaptureArchive::new(format!("{}/replay_captures",
                training_mod_consts::TRAINING_MODPACK_ROOT));
            // The same serialized owner is the only writer of mod presets. Native saves are read-only.
            #[cfg(feature = "mii-ordinary-cpu-alpha")]
            let mut presets = training_mod_consts::mii::draft_store::Store::open(
                training_mod_consts::TRAINING_MODPACK_ROOT).map_err(|error| error.to_string());
            while let Ok(message) = receiver.recv() {
                match message {
                    WriteJob::Capture(saved) => {
                        match archive.save_with_miis(&saved.replay_id, &saved.replay_name, "13.0.5",
                            &saved.session, saved.miis.as_ref()) {
                            Ok(outcome) => {
                                println!("[ReplayCapture] {} id={} name={:?}",
                                    if outcome.created { "persisted" } else { "already_present" },
                                    outcome.id, saved.replay_name);
                                #[cfg(feature = "mii-ordinary-cpu-alpha")]
                                if let Some(miis) = saved.miis {
                                    crate::character_roster::mii_replay::committed(miis);
                                }
                            }
                            Err(error) => println!("[ReplayCapture] persistence_failed id={:?}: {}",
                                saved.replay_id, error),
                        }
                    }
                    #[cfg(feature = "mii-ordinary-cpu-alpha")]
                    WriteJob::Catalog(mut native, reply) => {
                        match &presets {
                            Ok(store) => native = native.with_library(store.library()),
                            Err(error) => native.findings.push(training_mod_consts::mii::inventory::Finding::PresetSource {
                                error: format!("Mii preset library: {error}") }),
                        }
                        let _ = reply.send(native);
                    }
                    #[cfg(feature = "mii-ordinary-cpu-alpha")]
                    WriteJob::Import {name, configuration, native, reply} => {
                        let result = presets.as_mut().map_err(|error| error.clone()).and_then(|store| {
                            let name = name.unwrap_or_else(|| format!("Replay {:?}", configuration.canonical().class));
                            store.import(configuration, &name, &native).map_err(|error| error.to_string())?;
                            Ok(Arc::new(store.library().clone()))
                        });
                        let _ = reply.send(result);
                    }
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
pub fn persist_committed(replay_id: String, replay_name: String, session: Arc<LiveSession>,
    miis: Option<training_mod_consts::mii::replay::ReplayMiis>) {
    let Some(writer) = WRITER.get() else {
        println!("[ReplayCapture] persistence_writer_unavailable");
        return;
    };
    if let Err(error) = writer.try_send(WriteJob::Capture(Box::new(CommittedCapture { replay_id, replay_name, session, miis }))) {
        println!("[ReplayCapture] persistence_queue_unavailable: {}", error);
    }
}

/// Called only at the existing catalog file boundary (menu or owned catalog worker).
#[cfg(feature = "mii-ordinary-cpu-alpha")]
pub(crate) fn mii_catalog() -> training_mod_consts::mii::inventory::Snapshot {
    use training_mod_consts::mii::{inventory::Finding, saved};
    let mut native = saved::read_catalog(Path::new("save:/save_data/mii"));
    #[cfg(feature = "mii-editor-alpha")]
    crate::mii_editor_bridge::catalog_read(&native);
    let (reply, receive) = mpsc::channel();
    if let Some(writer) = WRITER.get() {
        // Startup opened the store before this queued snapshot; no initialization race.
        if writer.send(WriteJob::Catalog(native.clone(), reply)).is_ok() {
            if let Ok(catalog) = receive.recv() { return catalog; }
        }
    }
    native.findings.push(Finding::PresetSource {error: "Mii preset owner unavailable".into()});
    native
}

#[cfg(feature = "mii-ordinary-cpu-alpha")]
pub(crate) fn import_mii(configuration: training_mod_consts::mii::lifetime::Configuration,
    native: Arc<training_mod_consts::mii::inventory::Snapshot>,
    reply: mpsc::Sender<Result<Arc<training_mod_consts::mii::draft::Library>, String>>) -> Result<(), String> {
    WRITER.get().ok_or("Mii preset owner unavailable")?
        .try_send(WriteJob::Import {name: None, configuration, native, reply}).map_err(|error| error.to_string())
}

/// Called after the Training data directory and legacy migration are ready, and
/// before the Deluxe row registers. Configuration is read once; capture
/// sampling performs no filesystem reads.
pub fn init() {
    #[cfg(feature = "mii-ordinary-cpu-alpha")]
    if let Err(error) = start_writer() { println!("[ReplayCapture] Mii library unavailable: {error}"); }
    let path = REPLAY_CAPTURE_INI;
    let result = (|| -> Result<(CaptureSettings, Result<(), String>), String> {
        // The host persists by rename; a crash between its renames leaves only
        // `replay_capture.previous.ini`. Restore it before creating a default.
        match recover_previous_ini(Path::new(path)) {
            Ok(IniRecovery::Recovered) => println!("[ReplayCapture] replay_capture.ini restored from replay_capture.previous.ini"),
            Ok(IniRecovery::Present | IniRecovery::Absent) => {}
            Err(error) => println!("[ReplayCapture] replay_capture.previous.ini recovery skipped: {error}"),
        }
        match OpenOptions::new().write(true).create(true).create_new(true).truncate(false).open(path) {
            Ok(mut file) => file
                .write_all(DEFAULT_CAPTURE_INI.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|error| error.to_string())?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
        if fs::metadata(path).map_err(|error| error.to_string())?.len() > 16 * 1024 {
            return Err("replay_capture.ini exceeds 16 KiB".into());
        }
        let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
        let settings = CaptureSettings::parse(&text)?;
        Ok((settings, CaptureSettings::persistence_compatible(&text)))
    })();
    match result {
        Ok((settings, persistence)) => {
            SEED_ENABLED.store(settings.enabled, Ordering::Release);
            TOGGLE.seed(settings.enabled);
            match persistence {
                Ok(()) => {
                    PERSISTENCE_COMPATIBLE.store(true, Ordering::Release);
                    println!("[ReplayCapture] deluxe_persistence enabled");
                }
                Err(reason) => println!("[ReplayCapture] deluxe_persistence disabled: {reason}"),
            }
            crate::logging::diagnostic(format_args!("[ReplayCapture] live_enabled={}", settings.enabled));
        }
        Err(error) => {
            SEED_ENABLED.store(false, Ordering::Release);
            TOGGLE.seed(false);
            crate::logging::diagnostic(format_args!("[ReplayCapture] live capture disabled: {error}"));
        }
    }
}
