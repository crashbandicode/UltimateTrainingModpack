//! Diagnostic-only ImGui controls, rendered by Deluxe's existing Plugins page.
//! No game hooks, native object access, file I/O or blocking work in UI callbacks.
use deluxe_plugin_sdk::{OptionSpec, PluginHandler, PluginSpec, RequestStatus, Tone};

type Control = extern "C" fn(u32) -> u64;
const STATE: u32 = 0;
const START: u32 = 1;
const STOP: u32 = 2;
const CAP: u32 = 4;

static OPTIONS: [OptionSpec; 3] = [
    OptionSpec {
        no_persist: true,
        tones: Some(&[Tone::Neutral, Tone::Ok]),
        ..OptionSpec::new("capture", "Capture", &["Stopped", "Recording"])
    },
    OptionSpec {
        no_persist: true,
        read_only: true,
        tones: Some(&[Tone::Error, Tone::Warn, Tone::Ok, Tone::Warn, Tone::Neutral, Tone::Warn, Tone::Error]),
        ..OptionSpec::new("status", "Status", &["Unavailable", "Starting", "Recording", "Flushing", "Paused", "Limit reached", "Write error"])
    },
    OptionSpec {
        no_persist: true,
        read_only: true,
        ..OptionSpec::new("budget", "Limit", &["4 GiB"])
    },
];
static SPEC: PluginSpec = PluginSpec::new("training_trace", "Training Trace", &OPTIONS);

struct TraceHandler(Control);
impl PluginHandler for TraceHandler {
    fn current(&self, option: usize) -> Option<usize> {
        let state = (self.0)(STATE);
        match option {
            0 if (1..=4).contains(&state) => Some(usize::from(matches!(state, 1 | 2))),
            1 => Some(if state <= 6 { state as usize } else { 0 }),
            2 if (self.0)(CAP) > 0 => Some(0),
            _ => None,
        }
    }

    fn request(&self, option: usize, choice: usize) -> RequestStatus {
        if option != 0 || choice > 1 {
            return RequestStatus::Rejected;
        }
        match (self.0)(if choice == 1 { START } else { STOP }) {
            2 if choice == 1 => RequestStatus::Applied,
            4 if choice == 0 => RequestStatus::Applied,
            3 if choice == 1 => RequestStatus::Rejected,
            1..=4 => RequestStatus::Pending,
            5 | 6 => RequestStatus::Rejected,
            _ => RequestStatus::Unavailable,
        }
    }
}

#[cfg(target_os = "switch")]
fn register_when_loaded() {
    use deluxe_plugin_sdk::abi;
    use std::time::Duration;
    // Native NRO entry order is not assumed. Resolve on this short-lived worker,
    // never in an overlay callback, and do not cache a missing host forever.
    for _ in 0..40 {
        let mut control = 0usize;
        let mut host = 0usize;
        unsafe {
            skyline::nn::ro::LookupSymbol(&mut control, c"training_modpack_trace_control_v1".as_ptr().cast());
            skyline::nn::ro::LookupSymbol(&mut host, abi::API_SYMBOL_C.as_ptr());
        }
        if control != 0 && host != 0 {
            // Symbols are versioned C entry points owned by the process-lifetime
            // Training diagnostic and Deluxe plugins. No game addresses are used.
            let control = unsafe { std::mem::transmute::<usize, Control>(control) };
            let entry = unsafe { std::mem::transmute::<usize, abi::ApiEntryFn>(host) };
            let table = unsafe { entry() };
            if table.is_null() {
                skyline::println!("[TrainingTraceControl] Deluxe returned no plugin API table");
                return;
            }
            let table = unsafe { &*table };
            if !abi::table_matches(table.struct_size, table.abi_version) {
                skyline::println!("[TrainingTraceControl] incompatible Deluxe plugin API table");
                return;
            }
            let result = unsafe { deluxe_plugin_sdk::register_with(table, &SPEC, TraceHandler(control)) };
            skyline::println!("[TrainingTraceControl] registration: {:?}", result);
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    skyline::println!("[TrainingTraceControl] diagnostic recorder or Deluxe host unavailable");
}

#[cfg(target_os = "switch")]
#[skyline::main(name = "training_trace_control")]
pub fn main() {
    if let Err(error) = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(register_when_loaded)
    {
        skyline::println!("[TrainingTraceControl] initialization failed: {}", error);
    }
}

#[cfg(not(target_os = "switch"))]
pub fn descriptor_is_valid() -> bool {
    // Exercise the real SDK's descriptor validation on host builds without loading a host.
    matches!(deluxe_plugin_sdk::register(&SPEC, TraceHandler(unavailable)), Err(deluxe_plugin_sdk::RegisterError::HostMissing))
}
#[cfg(not(target_os = "switch"))]
extern "C" fn unavailable(_: u32) -> u64 { 0 }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    static COMMAND: AtomicU32 = AtomicU32::new(u32::MAX);
    extern "C" fn pending(command: u32) -> u64 {
        COMMAND.store(command, Ordering::Relaxed);
        match command { START => 1, STOP => 3, CAP => 4 * 1024 * 1024 * 1024 - 65536, _ => 4 }
    }
    extern "C" fn capped(_: u32) -> u64 { 5 }
    extern "C" fn stopping(_: u32) -> u64 { 3 }

    #[test]
    fn controls_send_commands_and_wait_for_flush() {
        let handler = TraceHandler(pending);
        assert_eq!(handler.request(0, 1), RequestStatus::Pending);
        assert_eq!(COMMAND.load(Ordering::Relaxed), START);
        assert_eq!(handler.request(0, 0), RequestStatus::Pending);
        assert_eq!(COMMAND.load(Ordering::Relaxed), STOP);
        assert_eq!(handler.request(1, 2), RequestStatus::Rejected);
        assert_eq!(COMMAND.load(Ordering::Relaxed), STOP);
        assert_eq!(handler.request(0, 2), RequestStatus::Rejected);
    }

    #[test]
    fn terminal_capture_cannot_be_started_or_shown_as_recording() {
        let handler = TraceHandler(capped);
        assert_eq!(handler.current(0), None);
        assert_eq!(handler.current(1), Some(5));
        assert_eq!(handler.request(0, 1), RequestStatus::Rejected);
    }

    #[test]
    fn start_during_flush_is_refused_instead_of_promising_a_deferred_start() {
        let handler = TraceHandler(stopping);
        assert_eq!(handler.request(0, 1), RequestStatus::Rejected);
        assert_eq!(handler.request(0, 0), RequestStatus::Pending);
    }

    #[test]
    #[cfg(not(target_os = "switch"))]
    fn actual_menu_descriptor_is_accepted_by_sdk() {
        assert!(descriptor_is_valid());
    }
}
