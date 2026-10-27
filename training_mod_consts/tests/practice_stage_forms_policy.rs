//! Compile the production pure policies with the real stage catalog and types.
//! This harness has no Skyline adapter, memory writes, hooks, or game process.

// The native adapter passes feature admission into the same pure policy tested
// here. No alpha feature or fake native implementation is needed in this crate.
#[path = "../../src/lobby_stage_cycle.rs"]
mod lobby_stage_cycle;

#[path = "../../src/practice_stage_request_policy.rs"]
mod practice_stage_request_policy;

// Ordinary Training entry rotation over the same cycle; adapter-only items are
// unused on the host, so allow them rather than mirroring them in the harness.
#[path = "../../src/training_stage_rotation_policy.rs"]
#[allow(dead_code)]
mod training_stage_rotation_policy;
