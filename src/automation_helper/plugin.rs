//! Reuse the installed overlay host; no renderer or drawing hooks are added.
use super::control::Controls;
use deluxe_plugin_sdk::{register, OptionSpec, PluginHandler, PluginSpec, RequestStatus, Tone};

pub static CONTROLS: Controls = Controls::new();

static OPTIONS: [OptionSpec; 4] = [
    OptionSpec::new("save_replay", "Save next replay", &["Off", "On"]),
    OptionSpec::new("rematch", "Rematch", &["Manual", "Yes", "No"]),
    OptionSpec::new("armed", "Run once", &["Off / Cancel", "Armed"]),
    OptionSpec {
        read_only: true,
        tones: Some(&[
            Tone::Neutral,
            Tone::Warn,
            Tone::Warn,
            Tone::Warn,
            Tone::Ok,
            Tone::Neutral,
            Tone::Error,
            Tone::Error,
        ]),
        ..OptionSpec::new(
            "status",
            "Status",
            &[
                "Idle",
                "Armed",
                "Saving",
                "Rematch",
                "Done",
                "Cancelled",
                "Failed",
                "Unavailable",
            ],
        )
    },
];
static SPEC: PluginSpec = PluginSpec::new("training_test_helper", "Test Helper", &OPTIONS);

struct Handler;
impl PluginHandler for Handler {
    fn current(&self, option: usize) -> Option<usize> {
        CONTROLS.current(option)
    }
    fn request(&self, option: usize, choice: usize) -> RequestStatus {
        if !CONTROLS.available() {
            return RequestStatus::Unavailable;
        }
        if CONTROLS.edit(option, choice) {
            RequestStatus::Applied
        } else {
            RequestStatus::Rejected
        }
    }
}

pub fn register_controls() {
    crate::logging::diagnostic(format_args!(
        "[TestHelper] host_registration={:?}",
        register(&SPEC, Handler)
    ));
}
