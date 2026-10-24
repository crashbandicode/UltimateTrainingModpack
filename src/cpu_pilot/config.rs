//! Explicit JSON input. Uses the repository's existing serde/serde_json crates.
use super::{Command, Config, Trigger};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum JsonTrigger {
    Immediate {},
    AfterMoves { moves: u32 },
    AfterSeconds { seconds: f64 },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JsonCommand {
    Release,
    WalkOff,
}

fn default_require_contact() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonConfig {
    pub level: u8,
    pub trigger: JsonTrigger,
    pub command: JsonCommand,
    #[serde(default = "default_require_contact")]
    pub require_contact: bool,
    pub guard_frames: u64,
}

impl TryFrom<JsonConfig> for Config {
    type Error = &'static str;
    fn try_from(json: JsonConfig) -> Result<Self, Self::Error> {
        let trigger = match json.trigger {
            JsonTrigger::Immediate {} => Trigger::Immediately,
            JsonTrigger::AfterMoves { moves } => Trigger::AfterCompletedMoves(moves),
            JsonTrigger::AfterSeconds { seconds } => {
                if !seconds.is_finite() || seconds < 0.0 {
                    return Err("cpu_seconds_must_be_finite_nonnegative");
                }
                let frames = (seconds * 60.0).ceil();
                if !frames.is_finite() || frames >= u64::MAX as f64 {
                    return Err("cpu_seconds_out_of_frame_range");
                }
                Trigger::AfterGameFrames(frames as u64)
            }
        };
        let config = Self {
            level: json.level,
            trigger,
            command: match json.command {
                JsonCommand::Release => Command::Release,
                JsonCommand::WalkOff => Command::WalkOff,
            },
            require_contact: json.require_contact,
            guard_frames: json.guard_frames,
        };
        config.validate()?;
        Ok(config)
    }
}

pub fn from_json(input: &str) -> Result<Config, String> {
    let json: JsonConfig = serde_json::from_str(input).map_err(|error| error.to_string())?;
    Config::try_from(json).map_err(str::to_string)
}
