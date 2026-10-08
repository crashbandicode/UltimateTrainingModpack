pub use log::{error, info, warn};
use log::{Level, LevelFilter, Metadata, Record, SetLoggerError};
use owo_colors::OwoColorize;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const DIAGNOSTIC_PATH: &str = "sd:/ultimate/TrainingModpack/practice-diagnostics.log";
const PREVIOUS_PATH: &str = "sd:/ultimate/TrainingModpack/practice-diagnostics.previous.log";
const DIAGNOSTIC_LIMIT: u64 = 128 * 1024;

struct DiagnosticLog {
    file: Option<File>,
    bytes: u64,
    sequence: u64,
    initialized: bool,
    sync_warning: bool,
}

static DIAGNOSTICS: Mutex<DiagnosticLog> = Mutex::new(DiagnosticLog {
    file: None,
    bytes: 0,
    sequence: 0,
    initialized: false,
    sync_warning: false,
});

fn rotate_diagnostics(log: &mut DiagnosticLog) -> io::Result<()> {
    log.file.take();
    if fs::metadata(DIAGNOSTIC_PATH).is_ok() {
        match fs::remove_file(PREVIOUS_PATH) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        fs::rename(DIAGNOSTIC_PATH, PREVIOUS_PATH)?;
    }
    log.file = Some(
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(DIAGNOSTIC_PATH)?,
    );
    log.bytes = 0;
    Ok(())
}

/// Sparse, persistent breadcrumbs for console crash investigation. At most two
/// 128 KiB files; no per-frame writes. Failures never abort game initialization.
pub fn init_diagnostics() {
    if let Ok(mut log) = DIAGNOSTICS.lock() {
        if log.initialized {
            return;
        }
        log.initialized = true;
        if let Err(e) = fs::create_dir_all(training_mod_consts::TRAINING_MODPACK_ROOT)
            .and_then(|_| rotate_diagnostics(&mut log))
        {
            println!("[PracticeDiagnostics] SD log unavailable: {}", e);
        }
    }
    diagnostic(format_args!("[PracticeDiagnostics] adapter=shared-roster30 game=13.0.5 main=21450c647b8c5940ed5b68218ea128fd3c5d597d; session_start; max_files=2 max_bytes_each={}", DIAGNOSTIC_LIMIT));
}

pub fn diagnostic(args: fmt::Arguments<'_>) {
    println!("{}", args);
    // In particular, the panic hook must not wait for a lock held by a failed
    // operation. Console output remains available if this breadcrumb is skipped.
    let Ok(mut log) = DIAGNOSTICS.try_lock() else {
        return;
    };
    if log.file.is_none() {
        return;
    }
    log.sequence += 1;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let line = format!("{} #{} {}\n", timestamp, log.sequence, args);
    if log.bytes + line.len() as u64 > DIAGNOSTIC_LIMIT {
        if let Err(e) = rotate_diagnostics(&mut log) {
            println!("[PracticeDiagnostics] rotation failed: {}", e);
            log.file = None;
            return;
        }
    }
    let file = log.file.as_mut().unwrap();
    if let Err(e) = file.write_all(line.as_bytes()) {
        println!("[PracticeDiagnostics] write failed: {}", e);
        log.file = None;
        return;
    }
    // Flush at the boundary, before the next native operation can crash. Some
    // console filesystems may not implement sync; retain written data and report
    // that limitation once instead of treating it as a game error.
    let synced = file.sync_all();
    log.bytes += line.len() as u64;
    if let Err(e) = synced {
        if !log.sync_warning {
            println!("[PracticeDiagnostics] filesystem sync unavailable: {}", e);
            log.sync_warning = true;
        }
    }
}

struct TrainingModpackLogger;

impl log::Log for TrainingModpackLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            match record.level() {
                Level::Error => {
                    println!(
                        "[TrainingModpack] [{}] {}",
                        record.level().red(),
                        record.args()
                    );
                }
                Level::Warn => {
                    println!(
                        "[TrainingModpack] [{}] {}",
                        record.level().yellow(),
                        record.args()
                    );
                }
                Level::Info => {
                    println!(
                        "[TrainingModpack] [{}] {}",
                        record.level().cyan(),
                        record.args()
                    );
                }
                _ => {
                    println!("[TrainingModpack] [{}] {}", record.level(), record.args());
                }
            };
        }
    }

    fn flush(&self) {}
}

static LOGGER: TrainingModpackLogger = TrainingModpackLogger;

pub fn init_logger() -> Result<(), SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Info))
}
