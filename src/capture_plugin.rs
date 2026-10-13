//! Deluxe Plugins-page row for the live capture toggle (feature `deluxe-sdk`).
//!
//! Registered once at startup, between the INI seed and the native adapter
//! install. The host runs `current`/`request` on its overlay draw thread; they
//! only touch the packed toggle word and never the CAPTURE mutex, the
//! filesystem or the host API. Without a Deluxe host `register` returns
//! `HostMissing` and revision 27 behaviour is unchanged.

use std::ffi::CStr;
use std::sync::atomic::{AtomicBool, Ordering};

use deluxe_plugin_sdk::{negotiated_descriptor_version, register, OptionSpec, PluginHandler,
    PluginSpec, RegisterError, RequestStatus, Tone};
use training_mod_macros::{ToggleRequest, CAPTURE_CONFIG_KEY};

use crate::replay_store::{self, REPLAY_CAPTURE_INI};

static HOST_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Off is default text, On is green: the same tones as the SDK demo's `enabled` row.
static CAPTURE_TONES: [Tone; 2] = [Tone::Neutral, Tone::Ok];

static CAPTURE_OPTION: [OptionSpec; 1] = [OptionSpec {
    id: "capture",
    label: "Capture",
    choices: &["Off", "On"],
    config_key: Some(CAPTURE_CONFIG_KEY),
    values: Some(&["off", "on"]),
    no_persist: false,
    tones: Some(&CAPTURE_TONES),
    read_only: false,
}];

/// Host persistence into the INI Training reads at startup.
static PERSISTED_SPEC: PluginSpec = PluginSpec {
    id: "training_modpack",
    label: "Training",
    options: &CAPTURE_OPTION,
    config_path: Some(REPLAY_CAPTURE_INI),
};

/// Session-only fallback when the INI layout is not persistence-compatible.
static SESSION_ONLY_SPEC: PluginSpec = PluginSpec {
    id: "training_modpack",
    label: "Training",
    options: &CAPTURE_OPTION,
    config_path: None,
};

struct CaptureHandler;

impl PluginHandler for CaptureHandler {
    fn current(&self, _option: usize) -> Option<usize> {
        replay_store::toggle().current_choice()
    }

    fn request(&self, _option: usize, choice: usize) -> RequestStatus {
        match replay_store::toggle().request_choice(choice) {
            ToggleRequest::Applied => RequestStatus::Applied,
            ToggleRequest::Pending => RequestStatus::Pending,
            ToggleRequest::Rejected => RequestStatus::Rejected,
            ToggleRequest::Unavailable => RequestStatus::Unavailable,
        }
    }
}

/// True once a Deluxe host accepted the row. The native adapter then installs
/// its guarded hooks at startup even when the INI says off, because a later ON
/// choice could not install Skyline hooks.
pub fn host_registered() -> bool {
    HOST_REGISTERED.load(Ordering::Acquire)
}

pub fn register_toggle() {
    let persist = replay_store::persistence_compatible();
    let spec = if persist { &PERSISTED_SPEC } else { &SESSION_ONLY_SPEC };
    match register(spec, CaptureHandler) {
        Ok(handle) => {
            HOST_REGISTERED.store(true, Ordering::Release);
            let (status, build) = host_identity();
            crate::logging::diagnostic(format_args!(
                "[ReplayCapture] deluxe register: Ok handle={} descriptor={:?} persist={} host_status={:#x} host_build={:?}",
                handle.0, negotiated_descriptor_version(handle), persist, status, build
            ));
        }
        Err(RegisterError::HostMissing) => {
            crate::logging::diagnostic(format_args!("[ReplayCapture] deluxe register: HostMissing"))
        }
        Err(error) => {
            let (status, build) = host_identity();
            crate::logging::diagnostic(format_args!(
                "[ReplayCapture] deluxe register: {:?} host_status={:#x} host_build={:?}",
                error, status, build
            ));
        }
    }
}

fn host_identity() -> (u32, String) {
    let Some(api) = deluxe_plugin_sdk::host() else { return (0, String::new()) };
    // SAFETY: `host()` only returns a table accepted by `table_matches`. Both
    // functions take no arguments; `host_build` returns null or a NUL-terminated
    // string valid for the life of the process, read here once and bounded.
    unsafe {
        let status = (api.host_status)();
        let build = (api.host_build)();
        let build = if build.is_null() {
            String::new()
        } else {
            CStr::from_ptr(build).to_string_lossy().chars().take(64).collect()
        };
        (status, build)
    }
}
