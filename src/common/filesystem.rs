use std::fs::read_to_string;
use std::sync::mpsc::{self, Sender};
use std::sync::LazyLock;

use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};

use log::{error, info};

static WRITER: LazyLock<Sender<WriteMessage>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel::<WriteMessage>();
    // Revision 33: an explicit 64 KiB stack. The Switch std default is 2 MiB, taken from the game
    // heap for the whole session. Ordinary jobs carry strings serialized by the caller;
    // the optional one-shot preview job carries a boxed fixed-size owned trace.
    std::thread::Builder::new()
        .name("config-writer".into())
        .stack_size(0x10000)
        .spawn(move || {
            for job in rx {
                match job {
                    WriteMessage::Text { path, contents } => match std::fs::write(path, contents) {
                        Ok(()) => info!("Saved data to {}", path),
                        Err(e) => error!("Could not write data to {}: {e}", path),
                    },
                    #[cfg(feature = "mii-preview-observer")]
                    WriteMessage::MiiPreview(trace) => {
                        crate::mii_preview_observer::write_trace(&trace)
                    }
                    #[cfg(feature = "mii-editor-alpha")]
                    WriteMessage::MiiEditor(trace) => crate::mii_editor_bridge::write_trace(&trace),
                    #[cfg(feature = "mii-quit-observer")]
                    WriteMessage::MiiQuit(trace) => crate::mii_quit_observer::write_trace(&trace),
                    #[cfg(feature = "automation-results-observer")]
                    WriteMessage::ResultsObserver(trace) => {
                        crate::automation_helper::observer::write_trace(&trace)
                    }
                }
            }
        })
        .expect("Couldn't create the config writer thread");
    tx
});

pub fn init() {
    LazyLock::force(&WRITER);
    info!("Initialized writer thread");
}

pub fn read_toml<T: DeserializeOwned>(path: &str) -> Result<T> {
    let contents = read_to_string(path)?;
    let parsed = toml::from_str::<T>(&contents)?;
    Ok(parsed)
}

pub fn read_json<T: DeserializeOwned>(path: &str) -> Result<T> {
    let contents = read_to_string(path)?;
    let parsed = serde_json::from_str::<T>(&contents)?;
    Ok(parsed)
}

enum WriteMessage {
    Text {
        path: &'static str,
        contents: String,
    },
    #[cfg(feature = "mii-preview-observer")]
    MiiPreview(Box<crate::mii_preview_observer::Trace>),
    #[cfg(feature = "mii-quit-observer")]
    MiiQuit(Box<crate::mii_quit_observer::Trace>),
    #[cfg(feature = "mii-editor-alpha")]
    MiiEditor(Box<crate::mii_editor_bridge::Trace>),
    #[cfg(feature = "automation-results-observer")]
    ResultsObserver(Box<crate::automation_helper::observer::Trace>),
}

/// Single owned capture; queued at results teardown or the explicit capture limit.
#[cfg(feature = "automation-results-observer")]
pub(crate) fn write_results_observer(trace: Box<crate::automation_helper::observer::Trace>) {
    let _ = WRITER.send(WriteMessage::ResultsObserver(trace));
}

/// One owned diagnostic job, queued after native state dispatch. Writer is forced before hooks.
#[cfg(feature = "mii-preview-observer")]
pub(crate) fn write_mii_preview(trace: Box<crate::mii_preview_observer::Trace>) {
    // A disconnected writer leaves the immutable trace absent: no hook logging or retry.
    let _ = WRITER.send(WriteMessage::MiiPreview(trace));
}

/// One owned read-only Quit observation, queued after the native UI callback.
#[cfg(feature = "mii-quit-observer")]
pub(crate) fn write_mii_quit(trace: Box<crate::mii_quit_observer::Trace>) {
    let _ = WRITER.send(WriteMessage::MiiQuit(trace));
}

#[cfg(feature = "mii-editor-alpha")]
pub(crate) fn write_mii_editor(trace: Box<crate::mii_editor_bridge::Trace>) {
    let _ = WRITER.send(WriteMessage::MiiEditor(trace));
}

pub fn write_toml<T: Serialize>(path: &'static str, data: &T) {
    match toml::to_string_pretty(data) {
        Ok(contents) => send_write_message(path, contents),
        Err(e) => error!("Could not serialize data for {path}: {e}"),
    }
}

pub fn write_json<T: Serialize>(path: &'static str, data: &T) {
    match serde_json::to_string_pretty(data) {
        Ok(contents) => send_write_message(path, contents),
        Err(e) => error!("Could not serialize data for {path}: {e}"),
    }
}

fn send_write_message(path: &'static str, contents: String) {
    if let Err(e) = WRITER.send(WriteMessage::Text { path, contents }) {
        error!("Could not queue write to {path}: {e}");
    }
}
