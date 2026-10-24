// Host-only entry point for the helper's request/lifetime policy. This does not
// test native addresses, the overlay SDK, or the installed NRO.
#[path = "../../src/automation_helper/control.rs"]
mod control;
#[path = "../../src/automation_helper/flow.rs"]
mod flow;
#[path = "../../src/automation_helper/popup.rs"]
mod popup;
#[path = "../../src/automation_helper/recording.rs"]
mod recording;
