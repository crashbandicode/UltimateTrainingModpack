//! Explicit development-only capture and, separately, the native results
//! adapter. Ordinary builds enable neither. The two features must not combine:
//! they own the same results-tick offsets.

#[cfg(all(feature = "automation-results-observer", feature = "automation-results-native"))]
compile_error!(
    "results observer and native adapter both hook 0x150e5f0 and 0x150e9d0; enable only one"
);

#[cfg(feature = "automation-results-observer")]
pub(crate) mod observer;
#[cfg(feature = "automation-results-observer")]
mod recording;

#[cfg(feature = "automation-results-native")]
mod adapter;
#[cfg(feature = "automation-results-native")]
mod control;
#[cfg(feature = "automation-results-native")]
mod flow;
#[cfg(feature = "automation-results-native")]
pub(crate) mod native;
#[cfg(feature = "automation-results-native")]
mod plugin;
#[cfg(feature = "automation-results-native")]
mod popup;
#[cfg(feature = "automation-results-native")]
mod pulse;
