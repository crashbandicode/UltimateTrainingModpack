//! Compatibility path for the editor's existing keyboard/resume adapters.
//! Native allocation, callbacks and the single lease live in the shared module.
pub(super) use crate::native_keyboard::inline_keyboard::{active, Decision, Keyboard, Text};
