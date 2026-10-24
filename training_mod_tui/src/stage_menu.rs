//! Persisted stage choice is independent of the ordered rotation's ranks.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Rotation,
    Direct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub mode: Mode,
    /// Native catalog ID, never a page/cell index. Validated by the catalog owner.
    pub direct_stage: u32,
}

impl Default for Preferences {
    fn default() -> Self { Self { mode: Mode::Rotation, direct_stage: 311 } }
}
